//! Import of legacy store data from CSV and XLSX files.
//!
//! The rules from `docs/IMPORTS.md` are enforced here:
//!
//! * the file never leaves the machine and is never modified;
//! * only cell *values* are read from a workbook: no formula is evaluated and
//!   no macro is run (XLSX is parsed as a ZIP of XML parts, and string cells
//!   are read as text);
//! * nothing is written until the owner approves the batch, and the approval
//!   is recorded with the source checksum;
//! * rows that cannot be trusted are rejected with a row/column code rather
//!   than guessed, and a preview never contains a full customer name.
//!
//! The ZIP container is read directly (`stored` and `deflate` entries, which
//! is what Excel and LibreOffice emit), inflating with `flate2`, so no
//! spreadsheet library is pulled in for a single read-only path.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;


/// A parsed delimited file: header names plus data rows.
#[derive(Debug)]
pub struct ParsedTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// One row that survived validation, ready to be written.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProductImportRow {
    pub row_number: i64,
    pub name_ar: String,
    pub name_en: String,
    pub category: String,
    pub price_minor: i64,
    pub cost_minor: i64,
    pub size_code: String,
    pub color: String,
    pub sku: String,
    pub barcode: Option<String>,
    pub opening_qty: i64,
}

/// The validated outcome of a file, before anything is written.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub batch_id: String,
    pub source_kind: String,
    pub source_sha256: String,
    pub total_rows: usize,
    pub accepted_rows: usize,
    pub rejected_rows: usize,
    pub accepted: Vec<ProductImportRow>,
    pub errors: Vec<ImportRowError>,
}

/// One rejected row, with the reason a person can act on.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportRowError {
    pub row_number: i64,
    pub code: String,
    pub column: String,
}

// ---------------------------------------------------------------------------
// Delimited text
// ---------------------------------------------------------------------------

/// Parse delimited text, honouring quoted fields, embedded newlines, and a
/// leading UTF-8 BOM. The delimiter is detected once from the first line.
pub fn parse_delimited(input: &str, delimiter: Option<char>) -> Result<ParsedTable, String> {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    let delimiter = match delimiter {
        Some(explicit) => explicit,
        None => detect_delimiter(text)?,
    };
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut field = String::new();
    let mut record: Vec<String> = Vec::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if in_quotes {
            if character == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(character);
            }
            continue;
        }
        match character {
            '"' if field.is_empty() => in_quotes = true,
            '\\' if chars.peek() == Some(&delimiter) => {
                // A backslash before the delimiter keeps it inside the field.
                field.push(delimiter);
                chars.next();
            }
            c if c == delimiter => {
                record.push(std::mem::take(&mut field));
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            other => field.push(other),
        }
    }
    if in_quotes {
        return Err("IMPORT_FILE_QUOTE_UNBALANCED".to_string());
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records.retain(|row| row.iter().any(|cell| !cell.trim().is_empty()));
    if records.is_empty() {
        return Err("IMPORT_FILE_EMPTY".to_string());
    }
    let headers = records
        .remove(0)
        .into_iter()
        .map(|header| header.trim().to_string())
        .collect::<Vec<_>>();
    if headers.is_empty() || headers.iter().all(|header| header.is_empty()) {
        return Err("IMPORT_FILE_HEADER_MISSING".to_string());
    }
    Ok(ParsedTable {
        headers,
        rows: records,
    })
}

fn detect_delimiter(text: &str) -> Result<char, String> {
    let first_line = text.lines().next().unwrap_or_default();
    let candidates = [',', ';', '\t', '|'];
    let best = candidates
        .iter()
        .map(|candidate| (*candidate, first_line.matches(*candidate).count()))
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count > 0)
        .map(|(candidate, _)| candidate)
        .ok_or_else(|| "IMPORT_FILE_DELIMITER_UNKNOWN".to_string())?;
    Ok(best)
}

// ---------------------------------------------------------------------------
// XLSX (ZIP + XML, values only)
// ---------------------------------------------------------------------------

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Read every file of a ZIP container into memory, by name.
///
/// Only the two methods that spreadsheet software writes are supported:
/// `stored` (0) and `deflate` (8). Anything else is rejected rather than
/// partially read.
pub fn read_zip_entries(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    if bytes.len() < 22 {
        return Err("IMPORT_XLSX_INVALID".to_string());
    }
    // Locate the end-of-central-directory record, scanning back over the
    // optional comment (at most 64 KiB).
    let mut eocd = None;
    let search_start = bytes.len().saturating_sub(65_557);
    for offset in (search_start..=bytes.len() - 22).rev() {
        if read_u32(bytes, offset) == 0x0605_4b50 {
            eocd = Some(offset);
            break;
        }
    }
    let eocd = eocd.ok_or_else(|| "IMPORT_XLSX_INVALID".to_string())?;
    let entry_count = read_u16(bytes, eocd + 10) as usize;
    let directory_offset = read_u32(bytes, eocd + 16) as usize;
    if entry_count > 512 || directory_offset >= bytes.len() {
        return Err("IMPORT_XLSX_INVALID".to_string());
    }
    let mut entries = HashMap::new();
    let mut cursor = directory_offset;
    for _ in 0..entry_count {
        if cursor + 46 > bytes.len() || read_u32(bytes, cursor) != 0x0201_4b50 {
            return Err("IMPORT_XLSX_INVALID".to_string());
        }
        let method = read_u16(bytes, cursor + 10);
        let compressed_size = read_u32(bytes, cursor + 20) as usize;
        let name_len = read_u16(bytes, cursor + 28) as usize;
        let extra_len = read_u16(bytes, cursor + 30) as usize;
        let comment_len = read_u16(bytes, cursor + 32) as usize;
        let local_offset = read_u32(bytes, cursor + 42) as usize;
        let name_start = cursor + 46;
        let name_end = name_start + name_len;
        if name_end > bytes.len() {
            return Err("IMPORT_XLSX_INVALID".to_string());
        }
        let name = String::from_utf8_lossy(&bytes[name_start..name_end]).to_string();
        cursor = name_end + extra_len + comment_len;

        if local_offset + 30 > bytes.len() || read_u32(bytes, local_offset) != 0x0403_4b50 {
            return Err("IMPORT_XLSX_INVALID".to_string());
        }
        let local_name_len = read_u16(bytes, local_offset + 26) as usize;
        let local_extra_len = read_u16(bytes, local_offset + 28) as usize;
        let data_start = local_offset + 30 + local_name_len + local_extra_len;
        let data_end = data_start
            .checked_add(compressed_size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "IMPORT_XLSX_INVALID".to_string())?;
        let raw = &bytes[data_start..data_end];
        let content = match method {
            0 => raw.to_vec(),
            8 => {
                use std::io::Read;
                let mut out = Vec::new();
                flate2::read::DeflateDecoder::new(raw)
                    .read_to_end(&mut out)
                    .map_err(|_| "IMPORT_XLSX_INVALID".to_string())?;
                out
            }
            _ => return Err("IMPORT_XLSX_UNSUPPORTED_COMPRESSION".to_string()),
        };
        entries.insert(name, content);
    }
    Ok(entries)
}

fn xml_unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    if let Ok(code) = u32::from_str_radix(hex, 16) {
                        if let Some(character) = char::from_u32(code) {
                            out.push(character);
                        }
                    }
                } else if let Some(dec) = entity.strip_prefix('#') {
                    if let Ok(code) = dec.parse::<u32>() {
                        if let Some(character) = char::from_u32(code) {
                            out.push(character);
                        }
                    }
                } else {
                    out.push('&');
                    out.push_str(entity);
                    out.push(';');
                }
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Extract text runs of an XML part, keeping the enclosing tag names so the
/// caller can tell an cell value (`<t>`) from a formatting attribute.
fn text_of<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut values = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start..];
        let Some(gt) = after.find('>') else { break };
        if after[..gt].ends_with('/') {
            rest = &after[gt + 1..];
            continue;
        }
        let body = &after[gt + 1..];
        let Some(end) = body.find(&close) else { break };
        values.push(&body[..end]);
        rest = &body[end + close.len()..];
    }
    values
}

/// Read the first worksheet of an XLSX workbook as a table of values.
///
/// Shared strings are resolved, inline strings are read, and formula cells
/// contribute only their cached value (`<v>`), never the formula itself.
pub fn parse_xlsx(bytes: &[u8]) -> Result<ParsedTable, String> {
    let entries = read_zip_entries(bytes)?;
    let shared: Vec<String> = match entries.get("xl/sharedStrings.xml") {
        Some(xml) => {
            let xml = String::from_utf8_lossy(xml);
            let mut strings = Vec::new();
            for item in text_of(&xml, "si") {
                let runs = text_of(item, "t");
                strings.push(xml_unescape(&runs.concat()));
            }
            strings
        }
        None => Vec::new(),
    };
    let sheet_name = entries
        .keys()
        .filter(|name| name.starts_with("xl/worksheets/sheet"))
        .min_by_key(|name| name.len())
        .cloned()
        .ok_or_else(|| "IMPORT_XLSX_NO_SHEET".to_string())?;
    let sheet = entries
        .get(&sheet_name)
        .ok_or_else(|| "IMPORT_XLSX_NO_SHEET".to_string())?;
    let sheet = String::from_utf8_lossy(sheet);

    let mut records: Vec<Vec<String>> = Vec::new();
    for row_xml in text_of(&sheet, "row") {
        let mut cells: Vec<(usize, String)> = Vec::new();
        let mut rest = row_xml;
        while let Some(start) = rest.find("<c") {
            let after = &rest[start..];
            let Some(gt) = after.find('>') else { break };
            let attributes = &after[..gt];
            let body_start = gt + 1;
            let tail = &after[body_start..];
            let (body, next) = match tail.find("</c>") {
                Some(end) => (&tail[..end], &tail[end + 4..]),
                None => (tail, ""),
            };
            rest = next;
            let cell_type = attribute_value(attributes, "t").unwrap_or_default();
            let column = attribute_value(attributes, "r")
                .as_deref()
                .and_then(column_index)
                .unwrap_or(cells.len());
            let raw = if cell_type == "inlineStr" {
                text_of(body, "t").join("")
            } else {
                text_of(body, "v").join("")
            };
            let value = match cell_type.as_str() {
                "s" => raw
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| shared.get(index).cloned())
                    .unwrap_or_default(),
                _ => xml_unescape(&raw),
            };
            cells.push((column, value));
        }
        if cells.is_empty() {
            continue;
        }
        let width = cells.iter().map(|(column, _)| column + 1).max().unwrap_or(0);
        let mut row = vec![String::new(); width];
        for (column, value) in cells {
            if column < row.len() {
                row[column] = value;
            }
        }
        records.push(row);
    }
    records.retain(|row| row.iter().any(|cell| !cell.trim().is_empty()));
    if records.is_empty() {
        return Err("IMPORT_FILE_EMPTY".to_string());
    }
    let headers = records
        .remove(0)
        .into_iter()
        .map(|header| header.trim().to_string())
        .collect();
    Ok(ParsedTable {
        headers,
        rows: records,
    })
}

fn attribute_value(attributes: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = attributes.find(&needle)? + needle.len();
    let rest = &attributes[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Turn an `A1` style reference into a zero-based column index.
fn column_index(reference: &str) -> Option<usize> {
    let mut index = 0usize;
    let mut found = false;
    for character in reference.chars() {
        if character.is_ascii_alphabetic() {
            let digit = character.to_ascii_uppercase() as u8 - b'A' + 1;
            index = index.checked_mul(26)?.checked_add(digit as usize)?;
            found = true;
        } else {
            break;
        }
    }
    found.then(|| index - 1)
}

// ---------------------------------------------------------------------------
// Field mapping and validation
// ---------------------------------------------------------------------------

/// Header aliases, so an Arabic or English export both map without work.
fn aliases(field: &str) -> &'static [&'static str] {
    match field {
        "name_ar" => &["name_ar", "اسم الصنف", "الصنف", "الاسم", "arabic name"],
        "name_en" => &["name_en", "اسم انجليزي", "الاسم الانجليزي", "english name"],
        "category" => &["category", "الفئة", "التصنيف", "القسم"],
        "price" => &["price", "السعر", "سعر البيع", "سعر"],
        "cost" => &["cost", "التكلفة", "سعر التكلفة"],
        "size" => &["size", "المقاس", "مقاس"],
        "color" => &["color", "اللون", "لون"],
        "sku" => &["sku", "كود", "الكود", "كود الصنف"],
        "barcode" => &["barcode", "الباركود", "باركود"],
        "qty" => &["qty", "quantity", "الرصيد", "الكمية", "المخزون"],
        _ => &[],
    }
}

/// Normalise a header so `أ`/`ا`, `ة`/`ه`, and tatweel do not split a match.
fn normalize_header(header: &str) -> String {
    header
        .trim()
        .to_lowercase()
        .replace('\u{0640}', "")
        .replace(['أ', 'إ', 'آ'], "ا")
        .replace('ى', "ي")
        .replace('ة', "ه")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn column_for(headers: &[String], field: &str) -> Option<usize> {
    let wanted = aliases(field);
    headers.iter().position(|header| {
        let normalized = normalize_header(header);
        wanted
            .iter()
            .any(|alias| normalize_header(alias) == normalized)
    })
}

/// Parse an amount such as `1,250.50` or `١٢٥٠` into minor units.
///
/// Arabic-Indic digits are accepted because store owners often type them.
pub fn parse_amount_minor(value: &str) -> Result<i64, String> {
    let cleaned = value
        .chars()
        .map(|character| match character {
            '٠'..='٩' => char::from_u32(character as u32 - 0x0660 + '0' as u32).unwrap_or(character),
            '۰'..='۹' => char::from_u32(character as u32 - 0x06f0 + '0' as u32).unwrap_or(character),
            other => other,
        })
        .filter(|character| {
            !matches!(
                character,
                ',' | ' ' | '\u{a0}' | 'ج' | 'م' | 'E' | 'G' | 'P' | '$'
            )
        })
        .collect::<String>();
    if cleaned.is_empty() {
        return Err("AMOUNT_EMPTY".to_string());
    }
    let (whole, fraction) = match cleaned.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (cleaned.as_str(), ""),
    };
    if fraction.len() > 2 || !whole.chars().all(|c| c.is_ascii_digit()) {
        return Err("AMOUNT_INVALID".to_string());
    }
    if !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err("AMOUNT_INVALID".to_string());
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| "AMOUNT_INVALID".to_string())?
    };
    let mut fraction_padded = fraction.to_string();
    while fraction_padded.len() < 2 {
        fraction_padded.push('0');
    }
    let fraction: i64 = if fraction_padded.is_empty() {
        0
    } else {
        fraction_padded
            .parse()
            .map_err(|_| "AMOUNT_INVALID".to_string())?
    };
    whole
        .checked_mul(100)
        .and_then(|value| value.checked_add(fraction))
        .ok_or_else(|| "AMOUNT_INVALID".to_string())
}

fn parse_quantity(value: &str) -> Result<i64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    trimmed
        .parse::<i64>()
        .map_err(|_| "QUANTITY_INVALID".to_string())
        .and_then(|quantity| {
            if (0..=1_000_000).contains(&quantity) {
                Ok(quantity)
            } else {
                Err("QUANTITY_INVALID".to_string())
            }
        })
}

/// Build validated import rows from a parsed table.
///
/// A row is accepted only when the fields the ledger needs are present and
/// well formed; everything else is reported with a stable error code.
pub fn validate_products(
    table: &ParsedTable,
    batch_id: &str,
) -> (ImportPreview, Vec<ImportRowError>) {
    let mut errors = Vec::new();
    let mut accepted = Vec::new();
    let mut seen_skus = std::collections::HashSet::new();
    let columns = (
        column_for(&table.headers, "name_ar"),
        column_for(&table.headers, "name_en"),
        column_for(&table.headers, "category"),
        column_for(&table.headers, "price"),
        column_for(&table.headers, "cost"),
        column_for(&table.headers, "size"),
        column_for(&table.headers, "color"),
        column_for(&table.headers, "sku"),
        column_for(&table.headers, "barcode"),
        column_for(&table.headers, "qty"),
    );
    for (index, row) in table.rows.iter().enumerate() {
        let row_number = (index + 2) as i64;
        let cell = |position: Option<usize>| -> String {
            position
                .and_then(|position| row.get(position))
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        };
        let name_ar = cell(columns.0);
        let name_en = cell(columns.1);
        let category = cell(columns.2);
        let size = cell(columns.5);
        let color = cell(columns.6);
        let sku = cell(columns.7);
        if name_ar.is_empty() && name_en.is_empty() {
            errors.push(ImportRowError {
                row_number,
                code: "IMPORT_NAME_REQUIRED".to_string(),
                column: "name_ar".to_string(),
            });
            continue;
        }
        if category.is_empty() {
            errors.push(ImportRowError {
                row_number,
                code: "IMPORT_CATEGORY_REQUIRED".to_string(),
                column: "category".to_string(),
            });
            continue;
        }
        if size.is_empty() || color.is_empty() {
            errors.push(ImportRowError {
                row_number,
                code: "IMPORT_SIZE_COLOR_REQUIRED".to_string(),
                column: if size.is_empty() { "size" } else { "color" }.to_string(),
            });
            continue;
        }
        let price_minor = match parse_amount_minor(&cell(columns.3)) {
            Ok(value) => value,
            Err(code) => {
                errors.push(ImportRowError {
                    row_number,
                    code: format!("IMPORT_{code}"),
                    column: "price".to_string(),
                });
                continue;
            }
        };
        let cost_minor = match parse_amount_minor(&cell(columns.4)) {
            Ok(value) => value,
            Err(_) if cell(columns.4).is_empty() => 0,
            Err(code) => {
                errors.push(ImportRowError {
                    row_number,
                    code: format!("IMPORT_{code}"),
                    column: "cost".to_string(),
                });
                continue;
            }
        };
        let opening_qty = match parse_quantity(&cell(columns.9)) {
            Ok(value) => value,
            Err(code) => {
                errors.push(ImportRowError {
                    row_number,
                    code: format!("IMPORT_{code}"),
                    column: "qty".to_string(),
                });
                continue;
            }
        };
        let sku = if sku.is_empty() {
            format!("IMP-{batch_id}-{row_number}")
        } else {
            sku
        };
        if !seen_skus.insert(sku.to_uppercase()) {
            errors.push(ImportRowError {
                row_number,
                code: "IMPORT_SKU_DUPLICATE_IN_FILE".to_string(),
                column: "sku".to_string(),
            });
            continue;
        }
        let barcode = {
            let value = cell(columns.8);
            if value.is_empty() { None } else { Some(value) }
        };
        accepted.push(ProductImportRow {
            row_number,
            name_ar: if name_ar.is_empty() {
                name_en.clone()
            } else {
                name_ar
            },
            name_en: if name_en.is_empty() {
                cell(columns.0)
            } else {
                name_en
            },
            category,
            price_minor,
            cost_minor,
            size_code: size,
            color,
            sku,
            barcode,
            opening_qty,
        });
    }
    let preview = ImportPreview {
        batch_id: batch_id.to_string(),
        source_kind: "CSV".to_string(),
        source_sha256: String::new(),
        total_rows: table.rows.len(),
        accepted_rows: accepted.len(),
        rejected_rows: errors.len(),
        accepted,
        errors: errors.clone(),
    };
    (preview, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_delimiter_and_quotes() {
        let table = parse_delimited("a,b\n1,\"x,y\"\n", None).expect("parsed");
        assert_eq!(table.headers, vec!["a", "b"]);
        assert_eq!(table.rows[0][1], "x,y");
    }

    #[test]
    fn rejects_unbalanced_quotes() {
        assert_eq!(
            parse_delimited("a,b\n1,\"x\n", None).unwrap_err(),
            "IMPORT_FILE_QUOTE_UNBALANCED"
        );
    }

    #[test]
    fn parses_arabic_and_plain_amounts() {
        assert_eq!(parse_amount_minor("1,250.50").unwrap(), 125_050);
        assert_eq!(parse_amount_minor("1250").unwrap(), 125_000);
        assert_eq!(parse_amount_minor("١٥٠").unwrap(), 15_000);
        assert_eq!(parse_amount_minor("12.5").unwrap(), 1_250);
        assert!(parse_amount_minor("").is_err());
        assert!(parse_amount_minor("12.345").is_err());
    }

    #[test]
    fn maps_arabic_headers() {
        let table = parse_delimited(
            "اسم الصنف,الفئة,السعر,المقاس,اللون,كود,الرصيد\nقميص,قمصان,250.00,M,أحمر,SKU-1,4\n",
            None,
        )
        .expect("parsed");
        let preview = validate_products(&table, "batch").0;
        assert_eq!(preview.accepted_rows, 1);
        assert_eq!(preview.accepted[0].price_minor, 25_000);
        assert_eq!(preview.accepted[0].opening_qty, 4);
        assert_eq!(preview.accepted[0].sku, "SKU-1");
    }

    #[test]
    fn rejects_rows_without_size_or_color() {
        let table = parse_delimited("name_ar,category,price,size,color\nshirt,tops,250,M,\n", None)
            .expect("parsed");
        let (preview, errors) = validate_products(&table, "batch");
        assert_eq!(preview.accepted_rows, 0);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "IMPORT_SIZE_COLOR_REQUIRED");
        assert_eq!(errors[0].row_number, 2);
    }

    #[test]
    fn flags_duplicate_skus_inside_the_file() {
        let table = parse_delimited(
            "name_ar,category,price,size,color,sku\nshirt,tops,250,M,red,SKU-1\nshirt,tops,250,L,red,SKU-1\n",
            None,
        )
        .expect("parsed");
        let (preview, errors) = validate_products(&table, "batch");
        assert_eq!(preview.accepted_rows, 1);
        assert_eq!(errors[0].code, "IMPORT_SKU_DUPLICATE_IN_FILE");
    }

    #[test]
    fn converts_a_column_reference_to_an_index() {
        assert_eq!(column_index("A1"), Some(0));
        assert_eq!(column_index("B2"), Some(1));
        assert_eq!(column_index("AA9"), Some(26));
        assert_eq!(column_index("12"), None);
    }


    /// Build a minimal, stored (uncompressed) XLSX container around the cells,
    /// so the ZIP reader and the sheet reader are exercised end to end.
    fn stored_xlsx(shared: &[&str], rows: &[&str]) -> Vec<u8> {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let shared_xml = format!(
            "<sst>{}</sst>",
            shared
                .iter()
                .map(|value| format!("<si><t>{value}</t></si>"))
                .collect::<String>()
        );
        files.push(("xl/sharedStrings.xml".to_string(), shared_xml.into_bytes()));
        let sheet_xml = format!(
            "<worksheet><sheetData>{}</sheetData></worksheet>",
            rows.join("")
        );
        files.push((
            "xl/worksheets/sheet1.xml".to_string(),
            sheet_xml.into_bytes(),
        ));
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, body) in &files {
            let offset = out.len() as u32;
            let crc = crc32(body);
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(body);

            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        let directory_len = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&directory_len.to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (index, entry) in table.iter_mut().enumerate() {
            let mut value = index as u32;
            for _ in 0..8 {
                value = if value & 1 == 1 {
                    0xEDB8_8320 ^ (value >> 1)
                } else {
                    value >> 1
                };
            }
            *entry = value;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for byte in bytes {
            crc = table[((crc ^ *byte as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        crc ^ 0xFFFF_FFFF
    }

    #[test]
    fn reads_a_shared_string_workbook() {
        let workbook = stored_xlsx(
            &[
                "name_ar",
                "category",
                "price",
                "size",
                "color",
                "shirt",
                "tops",
                "red",
            ],
            &[
                "<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>1</v></c><c r=\"C1\" t=\"s\"><v>2</v></c><c r=\"D1\" t=\"s\"><v>3</v></c><c r=\"E1\" t=\"s\"><v>4</v></c></row>",
                "<row r=\"2\"><c r=\"A2\" t=\"s\"><v>5</v></c><c r=\"B2\" t=\"s\"><v>6</v></c><c r=\"C2\"><v>199.99</v></c><c r=\"D2\" t=\"inlineStr\"><is><t>L</t></is></c><c r=\"E2\" t=\"s\"><v>7</v></c></row>",
            ],
        );
        let table = parse_xlsx(&workbook).expect("xlsx parsed");
        assert_eq!(table.headers[0], "name_ar");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][2], "199.99");
        assert_eq!(table.rows[0][3], "L");
        let preview = validate_products(&table, "batch").0;
        assert_eq!(preview.accepted_rows, 1);
        assert_eq!(preview.accepted[0].name_ar, "shirt");
        assert_eq!(preview.accepted[0].price_minor, 19_999);
        assert_eq!(preview.accepted[0].size_code, "L");
        assert_eq!(preview.accepted[0].color, "red");
    }

    #[test]
    fn rejects_a_file_that_is_not_a_zip() {
        assert_eq!(
            read_zip_entries(b"not a workbook").unwrap_err(),
            "IMPORT_XLSX_INVALID"
        );
    }

    #[test]
    fn unescapes_xml_entities() {
        assert_eq!(xml_unescape("a&amp;b&#65;&#x62;"), "a&bAb");
        assert_eq!(xml_unescape("plain"), "plain");
    }
}
