//! Receipt rendering and printing.
//!
//! The receipt is built from data that is already stored in the encrypted
//! database, so printing never depends on a live sale session and a receipt
//! can always be reprinted.
//!
//! Two delivery paths are supported:
//!
//! * **Direct** — the rendered receipt is sent to the Windows default
//!   printer as raw bytes through the spooler (`winspool`). This is the path
//!   used by thermal receipt printers.
//! * **Dialog** — the receipt is written as a printable HTML file and handed
//!   to the shell, which opens the system print dialog. This path renders
//!   Arabic correctly on any printer, including ones that cannot accept raw
//!   byte streams.
//!
//! The rendering itself is pure and is unit tested on every platform; only
//! the spooler calls are Windows specific.

use serde::Serialize;

/// Width of a receipt in characters.
pub const WIDTH_58: usize = 32;
pub const WIDTH_80: usize = 48;

/// One line of a receipt.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ReceiptLine {
    pub product: String,
    pub quantity: u32,
    /// Unit price in minor units (piasters).
    pub unit_price_minor: i64,
    /// Discount applied to this line, in minor units.
    pub discount_minor: i64,
    /// Tax applied to this line, in minor units.
    pub tax_minor: i64,
    /// Line total in minor units.
    pub total_minor: i64,
    pub sku: String,
    pub size: String,
    pub color: String,
}

/// A payment applied to the invoice.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ReceiptPayment {
    pub method: String,
    pub amount_minor: i64,
}

/// Everything needed to render a receipt.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ReceiptData {
    pub invoice_no: String,
    pub branch_name_ar: String,
    pub branch_name_en: String,
    pub currency_code: String,
    pub cashier_name: String,
    pub created_at_utc: String,
    pub customer_name: Option<String>,
    pub subtotal_minor: i64,
    pub item_discount_minor: i64,
    pub invoice_discount_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub tax_rate_bps: i64,
    pub prices_include_tax: bool,
    pub lines: Vec<ReceiptLine>,
    pub payments: Vec<ReceiptPayment>,
}

/// How the receipt should reach the printer.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PrintMode {
    /// Send straight to the default Windows printer.
    Direct,
    /// Open the system print dialog with a rendered receipt.
    Dialog,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrintOutcome {
    pub printed: bool,
    pub mode: String,
    pub printer: Option<String>,
    pub detail: String,
}

/// Format an amount in minor units as a currency string.
///
/// `minor` is in piasters for EGP, so the value is divided by 100 and printed
/// with two decimals.
pub fn format_amount(minor: i64, currency_code: &str) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let absolute = minor.unsigned_abs();
    let whole = absolute / 100;
    let fraction = absolute % 100;
    format!("{sign}{whole}.{fraction:02} {currency_code}")
}

/// Split `text` into chunks of at most `width` characters.
///
/// Characters outside the basic multilingual plane are not split, so emoji and
/// some symbols stay intact.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if current.chars().count() >= width {
            lines.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Render the receipt as plain text, ready for a character printer.
pub fn render_text(receipt: &ReceiptData, width: usize) -> String {
    let mut out = String::new();
    let separator = "-".repeat(width);

    let branch = if receipt.branch_name_ar.is_empty() {
        receipt.branch_name_en.clone()
    } else {
        receipt.branch_name_ar.clone()
    };
    out.push_str(&centre(&branch, width));
    out.push('\n');
    if !receipt.branch_name_en.is_empty() && receipt.branch_name_en != branch {
        out.push_str(&centre(&receipt.branch_name_en, width));
        out.push('\n');
    }
    out.push_str(&centre(
        &format!("فاتورة / Invoice {}", receipt.invoice_no),
        width,
    ));
    out.push('\n');
    out.push_str(&separator);
    out.push('\n');
    out.push_str(&format!(
        "{:<16}{:>16}",
        "التاريخ / Date", receipt.created_at_utc
    ));
    out.push('\n');
    out.push_str(&format!(
        "{:<16}{:>16}",
        "الكاشير / Cashier", receipt.cashier_name
    ));
    out.push('\n');
    if let Some(customer) = &receipt.customer_name {
        out.push_str(&format!("{:<16}{:>16}", "العميل / Customer", customer));
        out.push('\n');
    }
    out.push_str(&separator);
    out.push('\n');

    for line in &receipt.lines {
        let title = format!("{} | {} | {}", line.product, line.size, line.color);
        for chunk in wrap(&title, width) {
            out.push_str(&chunk);
            out.push('\n');
        }
        let unit = format_amount(line.unit_price_minor, &receipt.currency_code);
        out.push_str(&format!(
            "  {:>3} x {:<12}{:>10}",
            line.quantity,
            unit,
            format_amount(line.total_minor, &receipt.currency_code)
        ));
        out.push('\n');
        if line.discount_minor > 0 {
            out.push_str(&format!(
                "  {:<28}{:>10}",
                "خصم / Discount",
                format_amount(-line.discount_minor, &receipt.currency_code)
            ));
            out.push('\n');
        }
        if line.tax_minor > 0 {
            out.push_str(&format!(
                "  {:<28}{:>10}",
                "ضريبة / Tax",
                format_amount(line.tax_minor, &receipt.currency_code)
            ));
            out.push('\n');
        }
    }

    out.push_str(&separator);
    out.push('\n');
    out.push_str(&format!(
        "{:<16}{:>16}",
        "المجموع / Subtotal",
        format_amount(receipt.subtotal_minor, &receipt.currency_code)
    ));
    out.push('\n');
    if receipt.item_discount_minor > 0 {
        out.push_str(&format!(
            "{:<16}{:>16}",
            "خصم المنتجات / Item disc.",
            format_amount(-receipt.item_discount_minor, &receipt.currency_code)
        ));
        out.push('\n');
    }
    if receipt.invoice_discount_minor > 0 {
        out.push_str(&format!(
            "{:<16}{:>16}",
            "خصم الفاتورة / Invoice disc.",
            format_amount(-receipt.invoice_discount_minor, &receipt.currency_code)
        ));
        out.push('\n');
    }
    out.push_str(&format!(
        "{:<16}{:>16}",
        "الضريبة / Tax",
        format_amount(receipt.tax_minor, &receipt.currency_code)
    ));
    out.push('\n');
    out.push_str(&separator);
    out.push('\n');
    out.push_str(&format!(
        "{:<16}{:>16}",
        "الإجمالي / TOTAL",
        format_amount(receipt.total_minor, &receipt.currency_code)
    ));
    out.push('\n');

    for payment in &receipt.payments {
        out.push_str(&format!(
            "{:<16}{:>16}",
            payment_label(&payment.method),
            format_amount(payment.amount_minor, &receipt.currency_code)
        ));
        out.push('\n');
    }

    out.push_str(&separator);
    out.push('\n');
    out.push_str(&centre("شكرًا لتعاملكم معنا / Thank you", width));
    out.push('\n');
    out
}

fn payment_label(method: &str) -> String {
    match method {
        "CASH" => "نقدي / Cash".to_string(),
        "CARD" => "بطاقة / Card".to_string(),
        "WALLET" => "محفظة / Wallet".to_string(),
        "STORE_CREDIT" => "رصيد / Credit".to_string(),
        other => other.to_string(),
    }
}

/// Centre `text` inside `width` columns.
fn centre(text: &str, width: usize) -> String {
    let length = text.chars().count();
    if length >= width {
        return text.chars().take(width).collect();
    }
    let padding = (width - length) / 2;
    format!("{}{}", " ".repeat(padding), text)
}

/// Render the receipt as a printable HTML document.
///
/// The HTML is self contained so it can be printed without network access.
pub fn render_html(receipt: &ReceiptData, width: usize) -> String {
    let paper = if width <= WIDTH_58 { "58mm" } else { "80mm" };
    let mut rows = String::new();
    for line in &receipt.lines {
        rows.push_str(&format!(
            "<tr><td class=\"desc\">{}<br><small>{} · {} · {}</small></td>\
             <td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            escape_html(&line.product),
            escape_html(&line.size),
            escape_html(&line.color),
            escape_html(&line.sku),
            line.quantity,
            escape_html(&format_amount(line.total_minor, &receipt.currency_code)),
        ));
    }
    let mut payments = String::new();
    for payment in &receipt.payments {
        payments.push_str(&format!(
            "<tr><td>{}</td><td class=\"num\">{}</td></tr>",
            escape_html(&payment_label(&payment.method)),
            escape_html(&format_amount(payment.amount_minor, &receipt.currency_code)),
        ));
    }
    format!(
        "<!doctype html>\n<html lang=\"ar\" dir=\"rtl\"><head><meta charset=\"utf-8\">\
         <title>Invoice {invoice_no}</title><style>\
         @page {{ size: {paper} auto; margin: 4mm; }}\
         body {{ font-family: 'Segoe UI', Tahoma, sans-serif; font-size: 11px; margin: 0; }}\
         h1 {{ font-size: 15px; margin: 0 0 2px; text-align: center; }}\
         h2 {{ font-size: 11px; margin: 0 0 10px; text-align: center; font-weight: normal; }}\
         table {{ width: 100%; border-collapse: collapse; }}\
         td {{ padding: 2px 3px; vertical-align: top; }}\
         td.num {{ text-align: left; white-space: nowrap; }}\
         small {{ color: #555; font-size: 9px; }}\
         .meta td {{ border-bottom: 1px solid #ddd; }}\
         .totals td {{ border-top: 1px solid #333; font-weight: bold; }}\
         .foot {{ text-align: center; margin-top: 10px; font-size: 10px; }}\
         </style></head><body>\
         <h1>{branch}</h1><h2>فاتورة / Invoice {invoice_no}</h2>\
         <table class=\"meta\">\
         <tr><td>التاريخ / Date</td><td class=\"num\">{created_at}</td></tr>\
         <tr><td>الكاشير / Cashier</td><td class=\"num\">{cashier}</td></tr>\
         {customer_row}\
         </table><br>\
         <table><thead><tr><th>الصنف / Item</th><th class=\"num\">كمية / Qty</th>\
         <th class=\"num\">الإجمالي / Total</th></tr></thead><tbody>{rows}</tbody></table>\
         <br><table class=\"totals\">\
         <tr><td>المجموع / Subtotal</td><td class=\"num\">{subtotal}</td></tr>\
         {discount_row}\
         <tr><td>الضريبة / Tax</td><td class=\"num\">{tax}</td></tr>\
         <tr><td>الإجمالي / TOTAL</td><td class=\"num\">{total}</td></tr>\
         </table><br><table>{payments}</table>\
         <div class=\"foot\">شكرًا لتعاملكم معنا / Thank you</div>\
         </body></html>",
        invoice_no = escape_html(&receipt.invoice_no),
        branch = escape_html(&receipt.branch_name_ar),
        created_at = escape_html(&receipt.created_at_utc),
        cashier = escape_html(&receipt.cashier_name),
        customer_row = receipt
            .customer_name
            .as_ref()
            .map(|name| format!(
                "<tr><td>العميل / Customer</td><td class=\"num\">{}</td></tr>",
                escape_html(name)
            ))
            .unwrap_or_default(),
        rows = rows,
        discount_row = discount_rows(receipt),
        subtotal = escape_html(&format_amount(
            receipt.subtotal_minor,
            &receipt.currency_code
        )),
        tax = escape_html(&format_amount(receipt.tax_minor, &receipt.currency_code)),
        total = escape_html(&format_amount(receipt.total_minor, &receipt.currency_code)),
        payments = payments,
    )
}

fn discount_rows(receipt: &ReceiptData) -> String {
    let mut rows = String::new();
    if receipt.item_discount_minor > 0 {
        rows.push_str(&format!(
            "<tr><td>خصم المنتجات / Item discount</td><td class=\"num\">{}</td></tr>",
            escape_html(&format_amount(
                -receipt.item_discount_minor,
                &receipt.currency_code
            ))
        ));
    }
    if receipt.invoice_discount_minor > 0 {
        rows.push_str(&format!(
            "<tr><td>خصم الفاتورة / Invoice discount</td><td class=\"num\">{}</td></tr>",
            escape_html(&format_amount(
                -receipt.invoice_discount_minor,
                &receipt.currency_code
            ))
        ));
    }
    rows
}

/// Escape the characters that would otherwise be interpreted as HTML.
pub fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Windows spooler
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows_spooler {
    use std::{ffi::c_void, ptr};

    #[link(name = "Winspool")]
    unsafe extern "system" {
        fn GetDefaultPrinterW(p_buffer: *mut u16, p_size: *mut u32) -> i32;
        fn OpenPrinterW(
            p_printer_name: *const u16,
            ph_printer: *mut isize,
            p_default: *mut c_void,
        ) -> i32;
        fn StartDocPrinterW(h_printer: isize, p_doc_info: *mut c_void, p_job_id: *mut u32) -> i32;
        fn WritePrinter(
            h_printer: isize,
            p_buffer: *const c_void,
            cb_buffer: u32,
            p_written: *mut u32,
        ) -> i32;
        fn EndDocPrinter(h_printer: isize) -> i32;
        fn ClosePrinter(h_printer: isize) -> i32;
    }

    #[repr(C)]
    struct DocInfo1W {
        p_doc_name: *mut u16,
        p_output_file: *mut u16,
        p_datatype: *mut u16,
    }

    /// Name of the default Windows printer.
    pub fn default_printer() -> Result<String, String> {
        let mut size: u32 = 0;
        let ok = unsafe { GetDefaultPrinterW(ptr::null_mut(), &mut size) };
        if ok != 0 || size == 0 {
            return Err("PRINTER_NOT_FOUND".to_string());
        }
        let mut buffer = vec![0u16; size as usize];
        let ok = unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &mut size) };
        if ok == 0 {
            return Err("PRINTER_NOT_FOUND".to_string());
        }
        let text = String::from_utf16_lossy(&buffer);
        let name = text.trim_end_matches('\0').to_string();
        if name.is_empty() {
            return Err("PRINTER_NOT_FOUND".to_string());
        }
        Ok(name)
    }

    /// Send raw bytes to a named printer.
    pub fn print_raw(
        printer_name: &str,
        document_name: &str,
        payload: &[u8],
    ) -> Result<(), String> {
        let mut printer: isize = 0;
        let name: Vec<u16> = printer_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let ok = unsafe { OpenPrinterW(name.as_ptr(), &mut printer, ptr::null_mut()) };
        if ok == 0 {
            return Err("PRINTER_UNAVAILABLE".to_string());
        }
        let result = print_raw_with_handle(printer, document_name, payload);
        unsafe {
            ClosePrinter(printer);
        }
        result
    }

    fn print_raw_with_handle(
        printer: isize,
        document_name: &str,
        payload: &[u8],
    ) -> Result<(), String> {
        let mut doc_name: Vec<u16> = document_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut datatype: Vec<u16> = "RAW".encode_utf16().chain(std::iter::once(0)).collect();
        let mut doc_info = DocInfo1W {
            p_doc_name: doc_name.as_mut_ptr(),
            p_output_file: ptr::null_mut(),
            p_datatype: datatype.as_mut_ptr(),
        };
        let mut job_id: u32 = 0;
        let ok = unsafe {
            StartDocPrinterW(printer, &mut doc_info as *mut _ as *mut c_void, &mut job_id)
        };
        if ok == 0 {
            return Err("PRINTER_JOB_FAILED".to_string());
        }
        let mut written: u32 = 0;
        let ok = unsafe {
            WritePrinter(
                printer,
                payload.as_ptr() as *const c_void,
                payload.len() as u32,
                &mut written,
            )
        };
        unsafe {
            EndDocPrinter(printer);
        }
        if ok == 0 {
            return Err("PRINTER_WRITE_FAILED".to_string());
        }
        if written as usize != payload.len() {
            return Err("PRINTER_WRITE_INCOMPLETE".to_string());
        }
        Ok(())
    }
}

#[cfg(windows)]
pub use windows_spooler::{default_printer, print_raw};

/// Print a receipt on the default Windows printer.
#[cfg(windows)]
pub fn print_receipt(
    receipt: &ReceiptData,
    width: usize,
    mode: PrintMode,
) -> Result<PrintOutcome, String> {
    match mode {
        PrintMode::Direct => {
            let printer = default_printer()?;
            let payload = render_text(receipt, width);
            print_raw(
                &printer,
                &format!("Invoice {}", receipt.invoice_no),
                payload.as_bytes(),
            )?;
            Ok(PrintOutcome {
                printed: true,
                mode: "direct".to_string(),
                printer: Some(printer),
                detail: "PRINTED".to_string(),
            })
        }
        PrintMode::Dialog => print_via_dialog(receipt, width),
    }
}

/// Print a receipt on a non-Windows host.
///
/// There is no spooler to talk to, so the receipt is written to disk and the
/// caller is told where it is.
#[cfg(not(windows))]
pub fn print_receipt(
    receipt: &ReceiptData,
    width: usize,
    mode: PrintMode,
) -> Result<PrintOutcome, String> {
    let text = render_text(receipt, width);
    let directory = std::env::temp_dir();
    let path = directory.join(format!("naseej-receipt-{}.txt", receipt.invoice_no));
    std::fs::write(&path, text.as_bytes()).map_err(|_| "PRINT_WRITE_FAILED".to_string())?;
    Ok(PrintOutcome {
        printed: false,
        mode: match mode {
            PrintMode::Direct => "direct".to_string(),
            PrintMode::Dialog => "dialog".to_string(),
        },
        printer: None,
        detail: format!("PRINT_UNSUPPORTED_ON_THIS_PLATFORM:{path:?}"),
    })
}

/// Hand a rendered receipt to the shell, which opens the print dialog.
#[cfg(windows)]
fn print_via_dialog(receipt: &ReceiptData, width: usize) -> Result<PrintOutcome, String> {
    use std::{os::windows::ffi::OsStrExt, path::PathBuf};

    let directory = std::env::temp_dir().join("NaseejPOS").join("receipts");
    std::fs::create_dir_all(&directory).map_err(|_| "PRINT_DIR_FAILED".to_string())?;
    let path: PathBuf = directory.join(format!("receipt-{}.html", receipt.invoice_no));
    std::fs::write(&path, render_html(receipt, width).as_bytes())
        .map_err(|_| "PRINT_WRITE_FAILED".to_string())?;

    #[link(name = "Shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: isize,
            lp_operation: *const u16,
            lp_file: *const u16,
            lp_parameters: *const u16,
            lp_directory: *const u16,
            n_show_cmd: i32,
        ) -> isize;
    }

    let operation: Vec<u16> = "print".encode_utf16().chain(std::iter::once(0)).collect();
    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            0,
            operation.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            ptr::null(),
            1,
        )
    };
    // ShellExecute returns a value greater than 32 on success.
    if result as isize <= 32 {
        return Err("PRINT_DIALOG_FAILED".to_string());
    }
    Ok(PrintOutcome {
        printed: true,
        mode: "dialog".to_string(),
        printer: None,
        detail: "PRINT_DIALOG_OPENED".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_receipt() -> ReceiptData {
        ReceiptData {
            invoice_no: "INV-000123".to_string(),
            branch_name_ar: "الفرع الرئيسي".to_string(),
            branch_name_en: "Main branch".to_string(),
            currency_code: "EGP".to_string(),
            cashier_name: "Mariam Hassan".to_string(),
            created_at_utc: "2026-11-16T10:00:00Z".to_string(),
            customer_name: Some("Ahmed".to_string()),
            subtotal_minor: 100_00,
            item_discount_minor: 5_00,
            invoice_discount_minor: 0,
            tax_minor: 14_00,
            total_minor: 109_00,
            tax_rate_bps: 1400,
            prices_include_tax: false,
            lines: vec![ReceiptLine {
                product: "قميص".to_string(),
                quantity: 2,
                unit_price_minor: 50_00,
                discount_minor: 0,
                tax_minor: 14_00,
                total_minor: 100_00,
                sku: "SH-M-BL".to_string(),
                size: "M".to_string(),
                color: "أزرق".to_string(),
            }],
            payments: vec![ReceiptPayment {
                method: "CASH".to_string(),
                amount_minor: 109_00,
            }],
        }
    }

    #[test]
    fn formats_amounts_with_two_decimals() {
        assert_eq!(format_amount(100_00, "EGP"), "100.00 EGP");
        assert_eq!(format_amount(5, "EGP"), "0.05 EGP");
        assert_eq!(format_amount(-20_50, "EGP"), "-20.50 EGP");
        assert_eq!(format_amount(0, "EGP"), "0.00 EGP");
    }

    #[test]
    fn wraps_without_splitting_characters() {
        let wrapped = wrap("abcdef", 2);
        assert_eq!(wrapped, vec!["ab", "cd", "ef"]);
        assert_eq!(wrap("", 10), vec![String::new()]);
        assert_eq!(wrap("abc", 0), vec!["abc".to_string()]);
    }

    #[test]
    fn text_receipt_contains_the_essentials() {
        let receipt = sample_receipt();
        let text = render_text(&receipt, WIDTH_58);
        assert!(text.contains("INV-000123"));
        assert!(text.contains("109.00 EGP"));
        assert!(text.contains("قميص"));
        assert!(text.contains("نقدي / Cash"));
        for line in text.lines() {
            assert!(
                line.chars().count() <= WIDTH_58,
                "line longer than the paper: {line}"
            );
        }
    }

    #[test]
    fn html_receipt_escapes_untrusted_names() {
        let mut receipt = sample_receipt();
        receipt.customer_name = Some("<script>alert(1)</script>".to_string());
        let html = render_html(&receipt, WIDTH_58);
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn html_receipt_lists_every_line_and_payment() {
        let receipt = sample_receipt();
        let html = render_html(&receipt, WIDTH_80);
        assert!(html.contains("SH-M-BL"));
        assert!(html.contains("100.00 EGP"));
        assert!(html.contains("80mm"));
    }
}
