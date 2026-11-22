import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import OwnerPanel from './OwnerPanel';
import StoreDashboard, { type ManagerSection } from './StoreDashboard';
import type { Lang } from './lang';

/** The two colour themes. The dark theme swaps the CSS custom properties. */
type Theme = 'light' | 'dark';

const THEME_KEY = 'naseej-pos-theme';
type CategoryKey = 'all' | 'shirts' | 'trousers' | 'dresses' | 'jackets' | 'basics';
type PayMethod = 'CASH' | 'CARD' | 'WALLET';

type Variant = {
  id: string;
  size: string;
  color: string;
  colorAr: string;
  colorEn: string;
  colorHex: string;
  stock: number;
  sku: string;
  barcode: string;
  priceMinor: number;
};

type Product = {
  id: string;
  nameAr: string;
  nameEn: string;
  category: Exclude<CategoryKey, 'all'>;
  categoryAr: string;
  categoryEn: string;
  brand: string;
  priceMinor: number;
  badge?: 'new' | 'low' | 'best';
  art: string;
  variants: Variant[];
};

type CartLine = {
  variant: Variant;
  product: Product;
  quantity: number;
  discountBps: number;
};

type PaymentLine = { id: string; method: PayMethod; amountMinor: number };
type DesktopGate = 'checking' | 'license' | 'setup' | 'login' | 'tax' | 'ready' | 'error';
type AuthResult = { sessionToken: string; displayName: string; branchId: string };
type AppStatus = { needsOwnerSetup: boolean; taxConfigurationRequired: boolean; taxEnabled: boolean; taxRateBps: number; pricesIncludeTax: boolean | null };
type LicenseStatus = { valid: boolean; statusCode: string; deviceFingerprint: string | null; licenseId: string | null; customerId: string | null; planCode: string | null; expiresAtUtc: string | null; features: string[]; limits: Record<string, number>; publicKeyConfigured: boolean; localTrial: boolean; gracePeriod: boolean; graceExpiresAtUtc: string | null; productionBuild: boolean };
type ActivationRequest = { schema_version: number; request_id: string; product: string; app_version: string; requested_at_utc: string; device_fingerprint: string };
type WorkspaceTab = 'manager' | 'sale' | 'inventory' | 'products' | 'purchases' | 'reports' | 'users' | 'branches' | 'backup' | 'returns' | 'imports';
type PosTarget = 'sale' | 'products' | 'users' | 'reports' | 'branches' | 'backup';
type LoyaltySettings = { isEnabled: boolean; pointsPerCurrencyUnit: number; redeemValueMinor: number; minRedeemPoints: number };
type LoyaltyBalance = { customerId: string; points: number; valueMinor: number; settings: LoyaltySettings };
type StoreCreditRow = { id: string; customerId: string; issuedMinor: number; spentMinor: number; remainingMinor: number; returnDocumentId: string | null; createdAt: string };
type ImportRowError = { rowNumber: number; code: string; column: string };
type ImportedProductRow = { rowNumber: number; nameAr: string; nameEn: string; category: string; priceMinor: number; costMinor: number; sizeCode: string; color: string; sku: string; barcode: string | null; openingQty: number };
type ImportPreview = { batchId: string; sourceKind: string; sourceSha256: string; totalRows: number; acceptedRows: number; rejectedRows: number; accepted: ImportedProductRow[]; errors: ImportRowError[] };
type ImportCommitResult = { batchId: string; productsCreated: number; variantsCreated: number; stockMovements: number };
type ImportBatchRow = { id: string; sourceKind: string; status: string; totalRows: number; acceptedRows: number; rejectedRows: number; createdAt: string; committedAt: string | null };
type SetupOption = { id: string; code: string | null; nameAr: string; nameEn: string; colorHex: string | null; profileCode: string | null };
type CatalogSetupOptions = { categories: SetupOption[]; brands: SetupOption[]; seasons: SetupOption[]; sizes: SetupOption[]; colors: SetupOption[] };
type NativeInventoryRow = { variantId: string; sku: string; barcode: string | null; productNameAr: string; productNameEn: string; sizeCode: string; colorAr: string; colorEn: string; priceMinor: number; sellable: number; damaged: number; reorderLevel: number; needsReorder: boolean };
type SalesSummary = { invoiceCount: number; grossMinor: number; discountsMinor: number; taxMinor: number; returnsMinor: number; netMinor: number };
type ProfitSummary = { revenueMinor: number; costMinor: number; returnedMinor: number; restockedCostMinor: number; grossProfitMinor: number; marginBps: number };
type SupplierOption = { id: string; supplierCode: string; name: string };
type PurchaseOrderOpen = { id: string; orderNo: string; supplierName: string; status: string; lines: Array<{ id: string; sku: string; productNameAr: string; productNameEn: string; sizeCode: string; colorAr: string; quantityOpen: number; unitCostMinor: number }> };
type CustomerSummary = { id: string; name: string; phone: string | null; email: string | null; consentStatus: string; loyaltyPoints: number };
type UserOption = { id: string; username: string; displayName: string; roleCode: string; isActive: boolean };
type BranchOption = { id: string; code: string; nameAr: string; nameEn: string };
type BackupHistoryRow = { id: string; status: string; backupKind: string; filename: string | null; byteSize: number | null; startedAt: string; finishedAt: string | null };
type BackupOperationResult = { status: string; path: string; byteSize: number };
type ReturnPolicy = { periodDays: number; allowDiscountedItems: boolean; restockConditions: string[] };
type ReturnableInvoiceLine = { invoiceLineId: string; variantId: string; sku: string; productNameAr: string; productNameEn: string; size: string; color: string; quantity: number; returnedQuantity: number; remainingQuantity: number; lineTotalMinor: number };
type ReturnableInvoice = { invoiceId: string; invoiceNo: string; status: string; createdAt: string; lines: ReturnableInvoiceLine[] };
type ReturnSelection = { selected: boolean; quantity: string; itemCondition: string; disposition: string };
type NativeCatalogItem = { id: string; nameAr: string; nameEn: string; categoryAr: string; categoryEn: string; brand: string; priceMinor: number; variants: Array<{ id: string; size: string; color: string; colorAr: string; colorEn: string; colorHex: string; stock: number; sku: string; barcode: string; priceMinor: number }> };

const words = {
  ar: {
    sale: 'نقطة البيع', inventory: 'المخزون', products: 'الأصناف', purchases: 'المشتريات', reports: 'التقارير', users: 'المستخدمون', branches: 'الفروع', backup: 'النسخ الاحتياطي', returns: 'المرتجعات والاستبدال', imports: 'استيراد البيانات',
    title: 'نقطة البيع', subtitle: 'ابدأ فاتورة جديدة — امسح الباركود أو ابحث عن الصنف',
    search: 'ابحث بالاسم، SKU أو امسح الباركود...', searchHint: 'F2 للبحث',
    all: 'الكل', shirts: 'قمصان', trousers: 'بناطيل', dresses: 'فساتين', jackets: 'جاكيتات', basics: 'أساسيات',
    today: 'مبيعات اليوم', invoices: 'فواتير اليوم', average: 'متوسط الفاتورة',
    cart: 'سلة البيع', items: 'أصناف', empty: 'السلة فارغة', emptyHint: 'اختر صنفًا من الكتالوج لبدء البيع',
    discount: 'خصم الصنف', invoiceDiscount: 'خصم الفاتورة', discountReason: 'سبب الخصم (إلزامي)',
    customer: 'عميل اختياري', addCustomer: 'إضافة عميل', scanTip: 'الماسح جاهز',
    subtotal: 'الإجمالي قبل الخصم', discounts: 'الخصومات', tax: 'الضريبة (ضمن السعر)', total: 'الإجمالي المستحق',
    payment: 'طرق الدفع', addPayment: 'تقسيم الدفع', cash: 'نقدي', card: 'بطاقة', wallet: 'محفظة',
    remaining: 'المتبقي', complete: 'إتمام البيع', print: 'طباعة آخر فاتورة',
    chooseVariant: 'اختر المقاس واللون', size: 'المقاس', color: 'اللون', stock: 'المتاح', sku: 'SKU', barcode: 'باركود',
    addToCart: 'إضافة للسلة', cancel: 'إلغاء', new: 'جديد', low: 'كمية قليلة', best: 'الأكثر مبيعًا',
    cashier: 'الكاشير', shift: 'الوردية الصباحية', preview: 'معاينة تفاعلية',
    saleDone: 'تم حفظ الفاتورة محليًا', printFailed: 'تعذر الطباعة — الفاتورة محفوظة ويمكن إعادة طباعتها',
    needItems: 'أضف صنفًا أولًا', needPayment: 'مجموع وسائل الدفع يجب أن يساوي الإجمالي',
    reasonRequired: 'أدخل سبب الخصم اليدوي', outOfStock: 'الكمية المطلوبة غير متاحة لهذا المقاس واللون',
    searchEmpty: 'لا توجد نتائج مطابقة', selectAll: 'تصفية الأصناف', demoDisclaimer: 'بيانات تجريبية للواجهة فقط',
    paymentsNote: 'يمكن تقسيم الدفع بين النقدي والبطاقة والمحفظة', units: 'قطعة', currency: 'ج.م',
  },
  en: {
    sale: 'Point of sale', inventory: 'Inventory', products: 'Products', purchases: 'Purchases', reports: 'Reports', users: 'Users', branches: 'Branches', backup: 'Backup & restore', returns: 'Returns & exchanges', imports: 'Data import',
    title: 'Point of sale', subtitle: 'Start a new sale — scan a barcode or search the catalog',
    search: 'Search name, SKU, or scan a barcode...', searchHint: 'F2 to search',
    all: 'All items', shirts: 'Shirts', trousers: 'Trousers', dresses: 'Dresses', jackets: 'Jackets', basics: 'Basics',
    today: "Today's sales", invoices: "Today's invoices", average: 'Average ticket',
    cart: 'Sale cart', items: 'items', empty: 'Your cart is empty', emptyHint: 'Choose an item from the catalog to begin',
    discount: 'Item discount', invoiceDiscount: 'Invoice discount', discountReason: 'Discount reason (required)',
    customer: 'Optional customer', addCustomer: 'Add customer', scanTip: 'Scanner ready',
    subtotal: 'Subtotal before discount', discounts: 'Discounts', tax: 'Tax (included)', total: 'Amount due',
    payment: 'Payment methods', addPayment: 'Split payment', cash: 'Cash', card: 'Card', wallet: 'Wallet',
    remaining: 'Remaining', complete: 'Complete sale', print: 'Print last receipt',
    chooseVariant: 'Choose size and color', size: 'Size', color: 'Color', stock: 'In stock', sku: 'SKU', barcode: 'Barcode',
    addToCart: 'Add to cart', cancel: 'Cancel', new: 'New', low: 'Low stock', best: 'Best seller',
    cashier: 'Cashier', shift: 'Morning shift', preview: 'Interactive preview',
    saleDone: 'Sale saved locally', printFailed: 'Printer unavailable — sale is saved and can be reprinted',
    needItems: 'Add an item first', needPayment: 'Payment lines must equal the total due',
    reasonRequired: 'Enter a reason for the manual discount', outOfStock: 'That size and color combination is unavailable',
    searchEmpty: 'No matching products', selectAll: 'Filter products', demoDisclaimer: 'Demo data only',
    paymentsNote: 'Split between cash, card, and wallet', units: 'pcs', currency: 'EGP',
  },
} as const;

const colors = [
  { key: 'black', ar: 'أسود', en: 'Black', hex: '#292c32' },
  { key: 'sand', ar: 'رملي', en: 'Sand', hex: '#c5a783' },
  { key: 'blue', ar: 'أزرق سماوي', en: 'Sky blue', hex: '#8bb6d1' },
  { key: 'olive', ar: 'زيتي', en: 'Olive', hex: '#6b775d' },
  { key: 'cream', ar: 'عاجي', en: 'Ivory', hex: '#eee8da' },
  { key: 'rose', ar: 'وردي', en: 'Rose', hex: '#d9a6a7' },
];

function makeVariants(productId: string, skuRoot: string, sizes: string[], colorKeys: string[], priceMinor: number, stockBase: number): Variant[] {
  return sizes.flatMap((size, sizeIndex) => colorKeys.map((colorKey, colorIndex) => {
    const color = colors.find((c) => c.key === colorKey)!;
    const shortColor = colorKey.slice(0, 2).toUpperCase();
    const sku = `${skuRoot}-${size}-${shortColor}`;
    return {
      id: `${productId}-${size}-${colorKey}`,
      size,
      color: color.key,
      colorAr: color.ar,
      colorEn: color.en,
      colorHex: color.hex,
      stock: Math.max(0, stockBase - ((sizeIndex + colorIndex * 2) % 5)),
      sku,
      barcode: `NSJ${productId.replace(/\D/g, '').padStart(2, '0')}${String(sizeIndex + 1).padStart(2, '0')}${String(colorIndex + 1).padStart(2, '0')}`,
      priceMinor,
    };
  }));
}

const products: Product[] = [
  {
    id: 'p101', nameAr: 'قميص أكسفورد كلاسيكي', nameEn: 'Classic Oxford shirt', category: 'shirts', categoryAr: 'قمصان', categoryEn: 'Shirts',
    brand: 'NASEEJ', priceMinor: 89500, badge: 'best', art: 'art-blue', variants: makeVariants('p101', 'SH-OXF', ['S', 'M', 'L', 'XL'], ['blue', 'cream', 'black'], 89500, 8),
  },
  {
    id: 'p102', nameAr: 'بنطال دينم مستقيم', nameEn: 'Straight denim jeans', category: 'trousers', categoryAr: 'بناطيل', categoryEn: 'Trousers',
    brand: 'LINE 01', priceMinor: 129000, badge: 'low', art: 'art-denim', variants: makeVariants('p102', 'PT-DNM', ['28', '30', '32', '34'], ['blue', 'black'], 129000, 5),
  },
  {
    id: 'p103', nameAr: 'فستان صيفي من الكتان', nameEn: 'Linen summer dress', category: 'dresses', categoryAr: 'فساتين', categoryEn: 'Dresses',
    brand: 'NASEEJ', priceMinor: 164500, badge: 'new', art: 'art-rose', variants: makeVariants('p103', 'DR-LIN', ['S', 'M', 'L'], ['rose', 'sand', 'olive'], 164500, 6),
  },
  {
    id: 'p104', nameAr: 'جاكيت خفيف مبطن', nameEn: 'Light quilted jacket', category: 'jackets', categoryAr: 'جاكيتات', categoryEn: 'Jackets',
    brand: 'ATELIER', priceMinor: 239000, art: 'art-olive', variants: makeVariants('p104', 'JK-LGT', ['M', 'L', 'XL', 'XXL'], ['olive', 'black'], 239000, 4),
  },
  {
    id: 'p105', nameAr: 'تيشيرت قطن أساسي', nameEn: 'Essential cotton tee', category: 'basics', categoryAr: 'أساسيات', categoryEn: 'Basics',
    brand: 'NASEEJ', priceMinor: 59000, art: 'art-cream', variants: makeVariants('p105', 'TS-ESS', ['XS', 'S', 'M', 'L', 'XL'], ['cream', 'black', 'blue'], 59000, 11),
  },
  {
    id: 'p106', nameAr: 'بلوزة ساتان ناعمة', nameEn: 'Soft satin blouse', category: 'shirts', categoryAr: 'قمصان', categoryEn: 'Shirts',
    brand: 'ATELIER', priceMinor: 99000, badge: 'new', art: 'art-sand', variants: makeVariants('p106', 'BL-STN', ['S', 'M', 'L', 'XL'], ['sand', 'rose', 'black'], 99000, 7),
  },
  {
    id: 'p107', nameAr: 'قميص كتان واسع', nameEn: 'Relaxed linen shirt', category: 'shirts', categoryAr: 'قمصان', categoryEn: 'Shirts',
    brand: 'LINE 01', priceMinor: 112000, art: 'art-linen', variants: makeVariants('p107', 'SH-RLX', ['S', 'M', 'L', 'XL'], ['cream', 'olive', 'blue'], 112000, 4),
  },
  {
    id: 'p108', nameAr: 'بنطال قماش بقصة واسعة', nameEn: 'Wide-leg tailored pants', category: 'trousers', categoryAr: 'بناطيل', categoryEn: 'Trousers',
    brand: 'ATELIER', priceMinor: 145000, art: 'art-stone', variants: makeVariants('p108', 'PT-WID', ['28', '30', '32', '34'], ['black', 'sand', 'olive'], 145000, 6),
  },
];

const categoryLabels: Record<CategoryKey, keyof typeof words.ar> = {
  all: 'all', shirts: 'shirts', trousers: 'trousers', dresses: 'dresses', jackets: 'jackets', basics: 'basics',
};

function Icon({ name, size = 18 }: { name: string; size?: number }) {
  const paths: Record<string, string> = {
    bag: 'M5 8h14l1 12H4L5 8Zm3 0a4 4 0 0 1 8 0',
    box: 'M3 7.5 12 3l9 4.5v9L12 21l-9-4.5v-9ZM3 7.5l9 4.5 9-4.5M12 12v9',
    branch: 'M6 3v6a3 3 0 0 0 3 3h6a3 3 0 0 1 3 3v6M6 3v18M18 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6ZM6 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z',
    backup: 'M20 11a8 8 0 1 1-2.4-5.7M20 4v5h-5M12 7v5l3 2',
    return: 'M9 14 4 9l5-5M4 9h9a7 7 0 0 1 0 14h-2',
    grid: 'M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z',
    chart: 'M4 19V5m0 14h17M8 15l3-4 3 2 5-7',
    search: 'm20 20-4.2-4.2M18 10.5a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0Z',
    scan: 'M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3M7 12h10',
    plus: 'M12 5v14M5 12h14',
    minus: 'M5 12h14',
    close: 'M18 6 6 18M6 6l12 12',
    trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
    printer: 'M6 9V3h12v6M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2M6 14h12v7H6z',
    user: 'M20 21a8 8 0 0 0-16 0M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z',
    chevron: 'm9 18 6-6-6-6',
    check: 'm5 12 4 4L19 6',
    tag: 'M20 13 11 22l-9-9V2h11l9 9ZM7 7h.01',
    clock: 'M12 7v5l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z',
    credit: 'M2 8h20M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Zm2 9h4',
    wallet: 'M4 6.5A2.5 2.5 0 0 1 6.5 4H20v16H6.5A2.5 2.5 0 0 1 4 17.5v-11Zm0 1H20M16 13h.01',
    bell: 'M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9m-8 12h4',
    chevronDown: 'm6 9 6 6 6-6',
    warning: 'M12 3 1.5 21h21L12 3Zm0 6v6m0 3v.01',
    sun: 'M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10ZM12 1v2m0 18v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M1 12h2m18 0h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4',
    moon: 'M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z',
    hanger: 'M12 3.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Zm-.8 6h1.6v1.2L21 16.3c.9.6.4 1.9-.6 1.9H3.6C2.6 18.2 2.1 16.9 3 16.3l8.2-5.6V9.5Z',
    home: 'm3 11 9-8 9 8v8a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1v-8Z',
    inbox: 'M4 5h16v14H4zM4 13h5l1 2h4l1-2h5',
    archive: 'M4 5h16v4H4zM5 9v10h14V9M10 13h4',
    activity: 'M3 12h4l3-7 4 14 3-7h4',
    settings: 'M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4Zm0-9.7 1-2 2 .9.3 2.1 2 1.2 2-.7 1 1.7-1.5 1.5.1 2.2 1.8 1.2-1 1.8-2.2-.2-1.5 1.6-.8 2.1-2-.2-1-1.8-2.2.2-1.5-1.6-2.1.8-1-1.8 1.7-1.2-.1-2.2L7 6l1.8-1 1.5 1.5 2.1-.3Z',
    logout: 'M10 4H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h5M14 16l4-4-4-4M18 12H8',
    pin: 'M12 21s7-6.3 7-11a7 7 0 1 0-14 0c0 4.7 7 11 7 11Zm0-9.2a2 2 0 1 0 0-4 2 2 0 0 0 0 4Z',
    ruler: 'M4 17 17 4l3 3L7 20l-3-3Zm6-4 2 2m2-6 2 2',
    message: 'M20 15a3 3 0 0 1-3 3H9l-5 3V7a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v8Z',
  };
  return <svg aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d={paths[name] ?? paths.grid} /></svg>;
}

/** Approved policy: tax is rounded to the nearest whole EGP (100 piastres). */
function roundTaxToPolicy(minor: number) {
  return Math.round(minor / 100) * 100;
}

function money(minor: number, lang: Lang) {
  return new Intl.NumberFormat(lang === 'ar' ? 'ar-EG' : 'en-EG', {
    style: 'currency', currency: 'EGP', maximumFractionDigits: 0,
  }).format(minor / 100);
}

function localDateInput(date = new Date()) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function mapNativeCatalog(rows: NativeCatalogItem[]): Product[] {
  const artStyles = ['art-blue', 'art-denim', 'art-rose', 'art-olive', 'art-cream', 'art-sand', 'art-linen', 'art-stone'];
  return rows.map((row, index) => {
    const categoryName = row.categoryEn.toLocaleLowerCase();
    const category: Product['category'] = categoryName.includes('shirt') || categoryName.includes('blouse') ? 'shirts'
      : categoryName.includes('trouser') || categoryName.includes('pant') ? 'trousers'
      : categoryName.includes('dress') ? 'dresses'
      : categoryName.includes('jacket') || categoryName.includes('coat') ? 'jackets' : 'basics';
    return {
      id: row.id,
      nameAr: row.nameAr,
      nameEn: row.nameEn,
      category,
      categoryAr: row.categoryAr,
      categoryEn: row.categoryEn,
      brand: row.brand || '—',
      priceMinor: row.priceMinor,
      art: artStyles[index % artStyles.length],
      variants: row.variants.map((variant) => {
        const matched = colors.find((color) => color.key === variant.color || color.en.toLocaleLowerCase() === variant.colorEn.toLocaleLowerCase());
        return { ...variant, color: matched?.key ?? variant.color, colorHex: variant.colorHex || matched?.hex || '#aab4ac' };
      }),
    };
  });
}

/**
 * Open the owner panel with a long press on the application logo.
 *
 * A long press is a deliberate gesture, so the panel is out of the way of
 * the cashier but still reachable without a menu to hunt through.
 */
function useLongPress(onLongPress: () => void, delayMs = 1200) {
  const timer = useRef<number | null>(null);
  const holding = useRef(false);
  const [isHolding, setIsHolding] = useState(false);

  const clear = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    if (holding.current) {
      holding.current = false;
      setIsHolding(false);
    }
  }, []);

  const begin = useCallback(() => {
    holding.current = true;
    setIsHolding(true);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      holding.current = false;
      setIsHolding(false);
      onLongPress();
    }, delayMs);
  }, [delayMs, onLongPress]);

  useEffect(() => clear, [clear]);

  return {
    isHolding,
    handlers: {
      onPointerDown: begin,
      onPointerUp: clear,
      onPointerLeave: clear,
      onPointerCancel: clear,
      onContextMenu: (event: React.MouseEvent) => event.preventDefault(),
    },
  };
}

function App() {
  const [lang, setLang] = useState<Lang>('ar');
  const [category, setCategory] = useState<CategoryKey>('all');
  const [query, setQuery] = useState('');
  const [cart, setCart] = useState<CartLine[]>([]);
  const [activeProduct, setActiveProduct] = useState<Product | null>(null);
  const [selectedSize, setSelectedSize] = useState('');
  const [selectedColor, setSelectedColor] = useState('');
  const [invoiceDiscountBps, setInvoiceDiscountBps] = useState(0);
  const [discountReason, setDiscountReason] = useState('');
  const [payments, setPayments] = useState<PaymentLine[]>([{ id: 'pay-1', method: 'CASH', amountMinor: 0 }]);
  const [notice, setNotice] = useState<{ text: string; kind: 'success' | 'error' } | null>(null);
  const [salesToday, setSalesToday] = useState(724000);
  const [invoiceCount, setInvoiceCount] = useState(23);
  const [lastInvoice, setLastInvoice] = useState('—');
  const [lastInvoiceId, setLastInvoiceId] = useState<string | null>(null);
  const [isTauri, setIsTauri] = useState(() => Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__));
  const [gate, setGate] = useState<DesktopGate>(() => Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) ? 'checking' : 'ready');
  const [sessionToken, setSessionToken] = useState('');
  const [desktopName, setDesktopName] = useState('');
  const [catalogItems, setCatalogItems] = useState<Product[]>(products);
  const [activeTab, setActiveTab] = useState<WorkspaceTab>('manager');
  const [managerSection, setManagerSection] = useState<ManagerSection>('overview');
  const [catalogSetup, setCatalogSetup] = useState<CatalogSetupOptions | null>(null);
  const [inventoryRows, setInventoryRows] = useState<NativeInventoryRow[]>([]);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [workspaceError, setWorkspaceError] = useState('');
  const [productForm, setProductForm] = useState({ categoryId: '', brandId: '', seasonId: '', nameAr: '', nameEn: '', baseCost: '', basePrice: '', sizeIds: [] as string[], colorIds: [] as string[], openingQuantity: '0', reorderLevel: '0', openingReason: '' });
  const [adjustForm, setAdjustForm] = useState({ variantId: '', bucket: 'SELLABLE', quantityDelta: '1', reason: '' });
  const [salesSummary, setSalesSummary] = useState<SalesSummary | null>(null);
  const [profitSummary, setProfitSummary] = useState<ProfitSummary | null>(null);
  const [reportRange, setReportRange] = useState({ from: localDateInput(), to: localDateInput() });
  const [suppliers, setSuppliers] = useState<SupplierOption[]>([]);
  const [purchaseOrders, setPurchaseOrders] = useState<PurchaseOrderOpen[]>([]);
  const [supplierForm, setSupplierForm] = useState({ supplierCode: '', name: '', phone: '', email: '' });
  const [purchaseForm, setPurchaseForm] = useState({ supplierId: '', orderNo: '', variantId: '', quantity: '1', unitCost: '', taxRate: '0' });
  const [receiptForm, setReceiptForm] = useState({ purchaseOrderId: '', lineId: '', quantity: '1', supplierInvoiceRef: '' });
  const [selectedCustomer, setSelectedCustomer] = useState<CustomerSummary | null>(null);
  const [customerModalOpen, setCustomerModalOpen] = useState(false);
  const [customerForm, setCustomerForm] = useState({ name: '', phone: '', email: '', consentStatus: 'NOT_GIVEN' });
  const [customerMessage, setCustomerMessage] = useState('');
  const [users, setUsers] = useState<UserOption[]>([]);
  const [userForm, setUserForm] = useState({ username: '', displayName: '', password: '', roleCode: 'cashier' });
  const [branches, setBranches] = useState<BranchOption[]>([]);
  const [activeBranchId, setActiveBranchId] = useState('');
  const [branchForm, setBranchForm] = useState({ code: '', nameAr: '', nameEn: '' });
  const [branchAssignment, setBranchAssignment] = useState({ branchId: '', userId: '', roleCode: 'cashier' });
  const [backupHistory, setBackupHistory] = useState<BackupHistoryRow[]>([]);
  const [backupForm, setBackupForm] = useState({ destinationPath: '', password: '', destinationKind: 'LOCAL' });
  const [restoreForm, setRestoreForm] = useState({ sourcePath: '', password: '' });
  const [returnInvoiceNo, setReturnInvoiceNo] = useState('');
  const [returnPolicy, setReturnPolicy] = useState<ReturnPolicy | null>(null);
  const [returnableInvoice, setReturnableInvoice] = useState<ReturnableInvoice | null>(null);
  const [returnSelections, setReturnSelections] = useState<Record<string, ReturnSelection>>({});
  const [returnReason, setReturnReason] = useState('');
  const [refundMethod, setRefundMethod] = useState('CASH');
  const [exchangeVariantId, setExchangeVariantId] = useState('');
  const [exchangeQuantity, setExchangeQuantity] = useState('1');
  const [exchangePaymentMethod, setExchangePaymentMethod] = useState('CASH');
  const [exchangeRefundMethod, setExchangeRefundMethod] = useState('CASH');
  const [taxNeedsConfiguration, setTaxNeedsConfiguration] = useState(false);
  const [taxEnabled, setTaxEnabled] = useState(false);
  const [taxRateBps, setTaxRateBps] = useState(0);
  const [licenseStatus, setLicenseStatus] = useState<LicenseStatus | null>(null);
  const [licenseError, setLicenseError] = useState('');
  const [pricesIncludeTax, setPricesIncludeTax] = useState(false);
  const [authForm, setAuthForm] = useState({ username: '', displayName: '', password: '' });
  const [authError, setAuthError] = useState('');
  const [authBusy, setAuthBusy] = useState(false);
  const [taxRateInput, setTaxRateInput] = useState('14');
  const [taxEnabledInput, setTaxEnabledInput] = useState(false);
  const [loyaltySettings, setLoyaltySettings] = useState<LoyaltySettings>({ isEnabled: false, pointsPerCurrencyUnit: 1, redeemValueMinor: 100, minRedeemPoints: 0 });
  const [loyaltyForm, setLoyaltyForm] = useState({ enabled: false, pointsPerCurrencyUnit: '1', redeemValue: '1', minRedeemPoints: '0' });
  const [customerLoyalty, setCustomerLoyalty] = useState<LoyaltyBalance | null>(null);
  const [storeCredits, setStoreCredits] = useState<StoreCreditRow[]>([]);
  const [loyaltyPhone, setLoyaltyPhone] = useState('');
  const [importPreview, setImportPreview] = useState<ImportPreview | null>(null);
  const [importBatches, setImportBatches] = useState<ImportBatchRow[]>([]);
  const [importResult, setImportResult] = useState<ImportCommitResult | null>(null);
  const [ownerPanelOpen, setOwnerPanelOpen] = useState(false);
  const [taxInclusiveInput, setTaxInclusiveInput] = useState(false);
  const returnOperationRef = useRef<{ payload: string; key: string }>({ payload: '', key: '' });
  const searchRef = useRef<HTMLInputElement>(null);
  const t = words[lang];
  const dir = lang === 'ar' ? 'rtl' : 'ltr';
  const managerLabels: Record<ManagerSection, string> = {
    overview: lang === 'ar' ? 'نظرة عامة' : 'Overview',
    orders: lang === 'ar' ? 'الطلبات' : 'Orders',
    support: lang === 'ar' ? 'رسائل الدعم' : 'Support messages',
    activity: lang === 'ar' ? 'سجل النشاط' : 'Activity log',
  };
  const managerMode = activeTab === 'manager';
  const sectionTitle = managerMode ? managerLabels[managerSection] : t[activeTab];

  // The theme is a device preference, not a store setting: it is stored in the
  // browser profile so it survives restarts without touching the database.
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem(THEME_KEY);
    if (stored === 'light' || stored === 'dark') return stored;
    return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  });

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  const toggleTheme = () => setTheme((current) => (current === 'dark' ? 'light' : 'dark'));

  // The owner panel opens with a long press on the logo, in the sign in
  // screen and in the working screen alike.
  const openOwnerPanel = useCallback(() => setOwnerPanelOpen(true), []);
  const logoPress = useLongPress(openOwnerPanel);

  useEffect(() => {
    const native = Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
    setIsTauri(native);
    if (!native) {
      setGate('ready');
    } else {
      setSalesToday(0);
      setInvoiceCount(0);
      void bootDesktop().catch(() => setGate('error'));
    }
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'F2') {
        event.preventDefault();
        searchRef.current?.focus();
      }
      if (event.key === 'Escape') setActiveProduct(null);
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  async function bootDesktop() {
    const license = await invoke<LicenseStatus>('get_license_status');
    setLicenseStatus(license);
    if (!license.valid) { setGate('license'); return; }
    const status = await invoke<AppStatus>('app_status');
    setTaxRateBps(status.taxRateBps);
    setTaxEnabled(status.taxEnabled);
    setTaxEnabledInput(status.taxEnabled);
    setTaxNeedsConfiguration(false);
    if (status.pricesIncludeTax !== null) {
      setPricesIncludeTax(status.pricesIncludeTax);
      setTaxInclusiveInput(status.pricesIncludeTax);
    }
    setTaxRateInput(String(status.taxRateBps / 100));
    setGate(status.needsOwnerSetup ? 'setup' : 'login');
  }

  async function exportActivationRequest() {
    setLicenseError('');
    try {
      const request = await invoke<ActivationRequest>('create_activation_request');
      const blob = new Blob([JSON.stringify(request, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `naseej-activation-${request.request_id}.json`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (error) {
      setLicenseError(String(error));
    }
  }

  async function importLicenseFile(event: React.ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) return;
    setAuthBusy(true);
    setLicenseError('');
    try {
      const licenseJson = await file.text();
      await invoke<LicenseStatus>('import_activation_license', { licenseJson });
      await bootDesktop();
    } catch (error) {
      setLicenseError(String(error));
    } finally {
      setAuthBusy(false);
      event.target.value = '';
    }
  }

  const totals = useMemo(() => {
    const subtotal = cart.reduce((sum, line) => sum + line.variant.priceMinor * line.quantity, 0);
    const itemDiscount = cart.reduce((sum, line) => sum + Math.round(line.variant.priceMinor * line.quantity * line.discountBps / 10000), 0);
    const afterItemDiscount = Math.max(0, subtotal - itemDiscount);
    const invoiceDiscount = Math.round(afterItemDiscount * invoiceDiscountBps / 10000);
    const netAfterDiscount = Math.max(0, afterItemDiscount - invoiceDiscount);
    const tax = roundTaxToPolicy(pricesIncludeTax
      ? Math.round(netAfterDiscount * taxRateBps / (10000 + taxRateBps))
      : Math.round(netAfterDiscount * taxRateBps / 10000));
    const total = pricesIncludeTax ? netAfterDiscount : netAfterDiscount + tax;
    return { subtotal, itemDiscount, invoiceDiscount, discount: itemDiscount + invoiceDiscount, total, tax };
  }, [cart, invoiceDiscountBps, pricesIncludeTax, taxRateBps]);

  const paid = payments.reduce((sum, row) => sum + row.amountMinor, 0);
  const remaining = totals.total - paid;
  const manualDiscountUsed = invoiceDiscountBps > 0 || cart.some((line) => line.discountBps > 0);
  const selectedReturnLines = (returnableInvoice?.lines ?? []).flatMap((line) => {
    const selection = returnSelections[line.invoiceLineId];
    if (!selection?.selected) return [];
    return [{ line, selection, quantity: Math.trunc(Number(selection.quantity)) }];
  });
  const returnedValueMinor = selectedReturnLines.reduce((sum, selected) => {
    if (selected.quantity <= 0 || selected.quantity > selected.line.remainingQuantity) return sum;
    const before = Math.round(selected.line.lineTotalMinor * selected.line.returnedQuantity / selected.line.quantity);
    const after = Math.round(selected.line.lineTotalMinor * (selected.line.returnedQuantity + selected.quantity) / selected.line.quantity);
    return sum + after - before;
  }, 0);
  const allVariants = catalogItems.flatMap((product) => product.variants);
  const exchangeVariant = allVariants.find((variant) => variant.id === exchangeVariantId);
  const exchangeSubtotal = exchangeVariant ? exchangeVariant.priceMinor * Math.max(0, Math.trunc(Number(exchangeQuantity) || 0)) : 0;
  const exchangeTax = roundTaxToPolicy(pricesIncludeTax ? Math.round(exchangeSubtotal * taxRateBps / (10000 + taxRateBps)) : Math.round(exchangeSubtotal * taxRateBps / 10000));
  const exchangeTotal = pricesIncludeTax ? exchangeSubtotal : exchangeSubtotal + exchangeTax;
  const exchangeDifference = exchangeTotal - returnedValueMinor;

  function stableReturnIdempotencyKey(payload: unknown) {
    const signature = JSON.stringify(payload);
    if (returnOperationRef.current.payload !== signature) {
      returnOperationRef.current = { payload: signature, key: crypto.randomUUID() };
    }
    return returnOperationRef.current.key;
  }

  useEffect(() => {
    setPayments((current) => current.length === 1 ? [{ ...current[0], amountMinor: totals.total }] : current);
  }, [totals.total]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 3600);
    return () => window.clearTimeout(timer);
  }, [notice]);

  async function loadNativeCatalog(token: string) {
    const rows = await invoke<NativeCatalogItem[]>('get_catalog', { sessionToken: token });
    setCatalogItems(mapNativeCatalog(rows));
  }

  async function loadBranches(token: string) {
    const rows = await invoke<BranchOption[]>('get_branches', { sessionToken: token });
    setBranches(rows);
    if (!activeBranchId && rows[0]) setActiveBranchId(rows[0].id);
    return rows;
  }

  const importFileRef = useRef<HTMLInputElement>(null);

  function describeImportError(code: string, rowNumber: number) {
    const ar: Record<string, string> = {
      IMPORT_NAME_REQUIRED: 'اسم الصنف مطلوب',
      IMPORT_CATEGORY_REQUIRED: 'الفئة مطلوبة',
      IMPORT_SIZE_COLOR_REQUIRED: 'المقاس واللون مطلوبان لتوزيع المخزون',
      IMPORT_AMOUNT_INVALID: 'قيمة مالية غير صالحة',
      IMPORT_AMOUNT_EMPTY: 'خانة سعر فارغة',
      IMPORT_QUANTITY_INVALID: 'كمية غير صالحة',
      IMPORT_SKU_DUPLICATE_IN_FILE: 'كود مكرر داخل الملف',
    };
    const en: Record<string, string> = {
      IMPORT_NAME_REQUIRED: 'Item name is required',
      IMPORT_CATEGORY_REQUIRED: 'Category is required',
      IMPORT_SIZE_COLOR_REQUIRED: 'Size and color are required to place stock',
      IMPORT_AMOUNT_INVALID: 'Invalid amount',
      IMPORT_AMOUNT_EMPTY: 'Empty price cell',
      IMPORT_QUANTITY_INVALID: 'Invalid quantity',
      IMPORT_SKU_DUPLICATE_IN_FILE: 'Duplicate code inside the file',
    };
    const table = lang === 'ar' ? ar : en;
    return `${lang === 'ar' ? 'سطر' : 'Row'} ${rowNumber}: ${table[code] ?? code}`;
  }

  async function chooseImportFile(file: File | null) {
    if (!file) return;
    setWorkspaceBusy(true);
    setWorkspaceError('');
    setImportResult(null);
    try {
      const buffer = await file.arrayBuffer();
      let binary = '';
      const bytes = new Uint8Array(buffer);
      const chunk = 0x8000;
      for (let index = 0; index < bytes.length; index += chunk) {
        binary += String.fromCharCode(...bytes.subarray(index, index + chunk));
      }
      const preview = await invoke<ImportPreview>('preview_import', {
        sessionToken,
        fileName: file.name,
        contentBase64: btoa(binary),
        importType: 'PRODUCTS',
      });
      setImportPreview(preview);
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
      if (importFileRef.current) importFileRef.current.value = '';
    }
  }

  async function approveImport() {
    if (!importPreview) return;
    setWorkspaceBusy(true);
    setWorkspaceError('');
    try {
      const result = await invoke<ImportCommitResult>('commit_import', { sessionToken, batchId: importPreview.batchId });
      setImportResult(result);
      setImportPreview(null);
      setImportBatches(await invoke<ImportBatchRow[]>('get_import_batches', { sessionToken }));
      await loadNativeCatalog(sessionToken);
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function loadLoyalty(customerId: string) {
    try {
      const [balance, credits] = await Promise.all([
        invoke<LoyaltyBalance>('get_customer_loyalty', { sessionToken, customerId }),
        invoke<StoreCreditRow[]>('get_store_credit', { sessionToken, customerId }),
      ]);
      setCustomerLoyalty(balance);
      setStoreCredits(credits);
      setLoyaltySettings(balance.settings);
      setLoyaltyForm({
        enabled: balance.settings.isEnabled,
        pointsPerCurrencyUnit: String(balance.settings.pointsPerCurrencyUnit),
        redeemValue: (balance.settings.redeemValueMinor / 100).toString(),
        minRedeemPoints: String(balance.settings.minRedeemPoints),
      });
    } catch {
      setCustomerLoyalty(null);
      setStoreCredits([]);
    }
  }

  async function lookupLoyalty() {
    setWorkspaceBusy(true);
    setWorkspaceError('');
    try {
      const customer = await invoke<CustomerSummary | null>('get_customer_by_phone', { sessionToken, phone: loyaltyPhone.trim() });
      if (!customer) {
        setCustomerLoyalty(null);
        setStoreCredits([]);
        setWorkspaceError(lang === 'ar' ? 'لا يوجد عميل بهذا الرقم.' : 'No customer with that phone number.');
        return;
      }
      setSelectedCustomer(customer);
      await loadLoyalty(customer.id);
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function saveLoyaltySettings() {
    setWorkspaceBusy(true);
    setWorkspaceError('');
    try {
      const points = Number(loyaltyForm.pointsPerCurrencyUnit);
      const redeem = Math.round(Number(loyaltyForm.redeemValue.replace(',', '.')) * 100);
      const minimum = Number(loyaltyForm.minRedeemPoints);
      const saved = await invoke<LoyaltySettings>('configure_loyalty', {
        sessionToken,
        settings: {
          isEnabled: loyaltyForm.enabled,
          pointsPerCurrencyUnit: Number.isFinite(points) ? Math.round(points) : 0,
          redeemValueMinor: Number.isFinite(redeem) ? redeem : 100,
          minRedeemPoints: Number.isFinite(minimum) ? Math.round(minimum) : 0,
        },
      });
      setLoyaltySettings(saved);
      showNotice(lang === 'ar' ? 'تم حفظ إعدادات الولاء.' : 'Loyalty settings saved.', 'success');
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function openWorkspaceTab(tab: WorkspaceTab) {
    if (tab === 'manager') {
      setActiveTab('manager');
      setManagerSection('overview');
      return;
    }
    setActiveTab(tab);
    setWorkspaceError('');
    if (!isTauri) {
      if (tab === 'inventory') {
        setInventoryRows(catalogItems.flatMap((product) => product.variants.map((variant) => ({
          variantId: variant.id, sku: variant.sku, barcode: variant.barcode, productNameAr: product.nameAr, productNameEn: product.nameEn,
          sizeCode: variant.size, colorAr: variant.colorAr, colorEn: variant.colorEn, priceMinor: variant.priceMinor,
          sellable: variant.stock, damaged: 0, reorderLevel: 2, needsReorder: variant.stock <= 2,
        }))));
      }
      return;
    }
    setWorkspaceBusy(true);
    try {
      if (tab === 'inventory') {
        const rows = await invoke<NativeInventoryRow[]>('get_inventory', { sessionToken });
        setInventoryRows(rows);
        setAdjustForm((form) => ({ ...form, variantId: form.variantId || rows[0]?.variantId || '' }));
      } else if (tab === 'products') {
        const options = await invoke<CatalogSetupOptions>('get_catalog_setup_options', { sessionToken });
        setCatalogSetup(options);
        setProductForm((form) => ({ ...form, categoryId: form.categoryId || options.categories[0]?.id || '' }));
      } else if (tab === 'purchases') {
        const [supplierRows, orderRows, catalogRows] = await Promise.all([
          invoke<SupplierOption[]>('get_suppliers', { sessionToken }),
          invoke<PurchaseOrderOpen[]>('get_open_purchase_orders', { sessionToken }),
          invoke<NativeCatalogItem[]>('get_catalog', { sessionToken }),
        ]);
        setSuppliers(supplierRows);
        setPurchaseOrders(orderRows);
        const variants = catalogRows.flatMap((product) => product.variants);
        setPurchaseForm((form) => ({ ...form, supplierId: form.supplierId || supplierRows[0]?.id || '', variantId: form.variantId || variants[0]?.id || '', orderNo: form.orderNo || `PO-${localDateInput().replaceAll('-', '')}-${String(Date.now()).slice(-4)}` }));
        setReceiptForm((form) => ({ ...form, purchaseOrderId: form.purchaseOrderId || orderRows[0]?.id || '', lineId: form.lineId || orderRows[0]?.lines[0]?.id || '' }));
      } else if (tab === 'users') {
        setUsers(await invoke<UserOption[]>('list_users', { sessionToken }));
      } else if (tab === 'returns') {
        const [rows, policy] = await Promise.all([
          invoke<NativeCatalogItem[]>('get_catalog', { sessionToken }),
          invoke<ReturnPolicy>('get_return_policy', { sessionToken }),
        ]);
        const mapped = mapNativeCatalog(rows);
        setCatalogItems(mapped);
        setReturnPolicy(policy);
        const firstVariant = mapped.flatMap((product) => product.variants).find((variant) => variant.stock > 0);
        setExchangeVariantId((current) => current || firstVariant?.id || '');
      } else if (tab === 'branches') {
        const [branchRows, userRows] = await Promise.all([
          invoke<BranchOption[]>('get_branches', { sessionToken }),
          invoke<UserOption[]>('list_users', { sessionToken }),
        ]);
        setBranches(branchRows);
        setUsers(userRows);
        setBranchAssignment((form) => ({ ...form, branchId: form.branchId || activeBranchId || branchRows[0]?.id || '', userId: form.userId || userRows.find((user) => user.isActive)?.id || '' }));
      } else if (tab === 'backup') {
        setBackupHistory(await invoke<BackupHistoryRow[]>('get_backup_history', { sessionToken }));
      } else if (tab === 'imports') {
        setImportBatches(await invoke<ImportBatchRow[]>('get_import_batches', { sessionToken }));
      }
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function saveProduct(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!catalogSetup || !productForm.categoryId || !productForm.sizeIds.length || !productForm.colorIds.length) {
      setWorkspaceError(lang === 'ar' ? 'اختر الفئة ومقاسًا ولونًا واحدًا على الأقل.' : 'Select a category, at least one size, and at least one color.');
      return;
    }
    const toMinor = (value: string) => Math.round(Number(value.replace(',', '.')) * 100);
    const openingQuantity = Math.max(0, Math.trunc(Number(productForm.openingQuantity) || 0));
    const reorderLevel = Math.max(0, Math.trunc(Number(productForm.reorderLevel) || 0));
    const variants = productForm.sizeIds.flatMap((sizeId) => productForm.colorIds.map((colorId) => {
      const size = catalogSetup.sizes.find((option) => option.id === sizeId)!;
      const color = catalogSetup.colors.find((option) => option.id === colorId)!;
      const sku = `NSJ-${productForm.nameEn.replace(/[^A-Za-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 18)}-${size.code}-${color.code}`.toUpperCase();
      return { sizeId, colorId, sku, barcode: null, costOverrideMinor: null, priceOverrideMinor: null, reorderLevel, openingQuantity };
    }));
    if (variants.length > 200 || variants.some((variant) => !variant.sku || variant.sku.length > 80)) {
      setWorkspaceError(lang === 'ar' ? 'عدد التركيبات يتجاوز الحد أو أن SKU غير صالح.' : 'Too many size/color combinations or an invalid SKU.');
      return;
    }
    if (openingQuantity > 0 && !productForm.openingReason.trim()) {
      setWorkspaceError(lang === 'ar' ? 'سبب الرصيد الافتتاحي إلزامي.' : 'An opening-stock reason is required.');
      return;
    }
    if (!isTauri) {
      setWorkspaceError(lang === 'ar' ? 'إنشاء الصنف يتطلب تطبيق سطح المكتب وقاعدة البيانات المحلية.' : 'Product creation requires the desktop app and local database.');
      return;
    }
    setWorkspaceBusy(true);
    setWorkspaceError('');
    try {
      await invoke('create_product', {
        sessionToken,
        request: {
          idempotencyKey: crypto.randomUUID(), categoryId: productForm.categoryId,
          brandId: productForm.brandId || null, seasonId: productForm.seasonId || null,
          nameAr: productForm.nameAr, nameEn: productForm.nameEn,
          baseCostMinor: toMinor(productForm.baseCost), basePriceMinor: toMinor(productForm.basePrice),
          variants, openingReason: productForm.openingReason.trim() || null,
        },
      });
      await loadNativeCatalog(sessionToken);
      setProductForm((form) => ({ ...form, nameAr: '', nameEn: '', baseCost: '', basePrice: '', sizeIds: [], colorIds: [], openingQuantity: '0', openingReason: '' }));
      showNotice(lang === 'ar' ? 'تم حفظ الصنف ومتغيراته محليًا.' : 'Product and variants saved locally.', 'success');
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function submitStockAdjustment(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const quantityDelta = Math.trunc(Number(adjustForm.quantityDelta));
    if (!adjustForm.variantId || !Number.isFinite(quantityDelta) || quantityDelta === 0 || !adjustForm.reason.trim()) {
      setWorkspaceError(lang === 'ar' ? 'اختر SKU وأدخل فرق كمية وسببًا واضحًا.' : 'Choose a SKU and enter a non-zero quantity change with a reason.');
      return;
    }
    if (!isTauri) { setWorkspaceError(lang === 'ar' ? 'التسوية متاحة في تطبيق سطح المكتب فقط.' : 'Stock adjustments are available in the desktop app only.'); return; }
    setWorkspaceBusy(true);
    setWorkspaceError('');
    try {
      await invoke('adjust_inventory', { sessionToken, request: { variantId: adjustForm.variantId, bucket: adjustForm.bucket, quantityDelta, reason: adjustForm.reason.trim(), idempotencyKey: crypto.randomUUID() } });
      const rows = await invoke<NativeInventoryRow[]>('get_inventory', { sessionToken });
      setInventoryRows(rows);
      setAdjustForm((form) => ({ ...form, quantityDelta: '1', reason: '' }));
      showNotice(lang === 'ar' ? 'تم تسجيل حركة التسوية.' : 'Inventory adjustment recorded.', 'success');
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function createSupplier(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isTauri) { setWorkspaceError(lang === 'ar' ? 'إدارة الموردين متاحة في تطبيق سطح المكتب فقط.' : 'Supplier management is available in the desktop app only.'); return; }
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      await invoke('create_supplier', { sessionToken, request: { supplierCode: supplierForm.supplierCode.trim(), name: supplierForm.name.trim(), phone: supplierForm.phone.trim() || null, email: supplierForm.email.trim() || null, address: null, taxRegistration: null, notes: null } });
      setSuppliers(await invoke<SupplierOption[]>('get_suppliers', { sessionToken }));
      setSupplierForm({ supplierCode: '', name: '', phone: '', email: '' });
      showNotice(lang === 'ar' ? 'تم حفظ المورد.' : 'Supplier saved.', 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function createPurchaseOrder(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const quantity = Math.trunc(Number(purchaseForm.quantity));
    const unitCostMinor = Math.round(Number(purchaseForm.unitCost.replace(',', '.')) * 100);
    const taxRateBps = Math.round(Number(purchaseForm.taxRate.replace(',', '.')) * 100);
    if (!purchaseForm.supplierId || !purchaseForm.variantId || !purchaseForm.orderNo.trim() || quantity <= 0 || unitCostMinor < 0 || taxRateBps < 0 || taxRateBps > 10000) {
      setWorkspaceError(lang === 'ar' ? 'تحقق من المورد والصنف والكمية والتكلفة والضريبة.' : 'Check supplier, item, quantity, cost, and tax.'); return;
    }
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      await invoke('create_purchase_order', { sessionToken, request: { supplierId: purchaseForm.supplierId, orderNo: purchaseForm.orderNo.trim(), expectedAt: null, notes: null, lines: [{ variantId: purchaseForm.variantId, quantity, unitCostMinor, taxRateBps }] } });
      setPurchaseOrders(await invoke<PurchaseOrderOpen[]>('get_open_purchase_orders', { sessionToken }));
      setPurchaseForm((form) => ({ ...form, orderNo: `PO-${localDateInput().replaceAll('-', '')}-${String(Date.now()).slice(-4)}`, quantity: '1' }));
      showNotice(lang === 'ar' ? 'تم إنشاء أمر التوريد.' : 'Purchase order created.', 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function receiveGoods(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const order = purchaseOrders.find((item) => item.id === receiptForm.purchaseOrderId);
    const line = order?.lines.find((item) => item.id === receiptForm.lineId);
    const quantityReceived = Math.trunc(Number(receiptForm.quantity));
    if (!order || !line || quantityReceived <= 0 || quantityReceived > line.quantityOpen) {
      setWorkspaceError(lang === 'ar' ? 'تحقق من أمر الشراء والبند والكمية المتبقية.' : 'Check the purchase order, line, and remaining quantity.'); return;
    }
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      await invoke('receive_goods', { sessionToken, request: { purchaseOrderId: order.id, receiptNo: `GR-${Date.now()}`, supplierInvoiceRef: receiptForm.supplierInvoiceRef.trim() || null, idempotencyKey: crypto.randomUUID(), lines: [{ purchaseOrderLineId: line.id, quantityReceived }] } });
      setPurchaseOrders(await invoke<PurchaseOrderOpen[]>('get_open_purchase_orders', { sessionToken }));
      await loadNativeCatalog(sessionToken);
      setReceiptForm((form) => ({ ...form, quantity: '1', supplierInvoiceRef: '' }));
      showNotice(lang === 'ar' ? 'تم استلام الكمية وتحديث مخزون المقاس واللون.' : 'Goods received and variant-level stock updated.', 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function lookupCustomer(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isTauri) { setCustomerMessage(lang === 'ar' ? 'البحث عن العملاء يتطلب تطبيق سطح المكتب.' : 'Customer lookup requires the desktop app.'); return; }
    setWorkspaceBusy(true); setCustomerMessage('');
    try {
      const customer = await invoke<CustomerSummary | null>('get_customer_by_phone', { sessionToken, phone: customerForm.phone.trim() });
      if (!customer) { setCustomerMessage(lang === 'ar' ? 'لا يوجد عميل مطابق؛ يمكنك إنشاء سجل جديد.' : 'No matching customer. You can create a new profile.'); return; }
      setSelectedCustomer(customer);
      setCustomerMessage(lang === 'ar' ? `تم العثور على ${customer.name}.` : `Found ${customer.name}.`);
    } catch (error) { setCustomerMessage(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function createCustomer(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isTauri) { setCustomerMessage(lang === 'ar' ? 'حفظ العملاء يتطلب تطبيق سطح المكتب.' : 'Saving customer profiles requires the desktop app.'); return; }
    setWorkspaceBusy(true); setCustomerMessage('');
    try {
      const customer = await invoke<CustomerSummary>('create_customer', { sessionToken, request: { name: customerForm.name.trim(), phone: customerForm.phone.trim() || null, email: customerForm.email.trim() || null, notes: null, sizePreferences: null, consentStatus: customerForm.consentStatus } });
      setSelectedCustomer(customer);
      setCustomerMessage(lang === 'ar' ? 'تم إنشاء سجل العميل واختياره للفاتورة.' : 'Customer profile created and attached to the sale.');
    } catch (error) { setCustomerMessage(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function createUser(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      await invoke('create_user', { sessionToken, request: { username: userForm.username.trim(), displayName: userForm.displayName.trim(), password: userForm.password, roleCode: userForm.roleCode } });
      setUsers(await invoke<UserOption[]>('list_users', { sessionToken }));
      setUserForm({ username: '', displayName: '', password: '', roleCode: 'cashier' });
      showNotice(lang === 'ar' ? 'تم إنشاء الحساب.' : 'User account created.', 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function changeUserRole(userId: string, roleCode: string) {
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      await invoke('change_user_role', { sessionToken, request: { userId, roleCode } });
      setUsers(await invoke<UserOption[]>('list_users', { sessionToken }));
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function toggleUserActive(user: UserOption) {
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      await invoke('set_user_active', { sessionToken, request: { userId: user.id, isActive: !user.isActive } });
      setUsers(await invoke<UserOption[]>('list_users', { sessionToken }));
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function createBranch(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isTauri) { setWorkspaceError(lang === 'ar' ? 'إدارة الفروع متاحة في تطبيق سطح المكتب فقط.' : 'Branch administration requires the desktop app.'); return; }
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      const branch = await invoke<BranchOption>('create_branch', { sessionToken, request: { code: branchForm.code.trim(), nameAr: branchForm.nameAr.trim(), nameEn: branchForm.nameEn.trim() } });
      setBranches(await invoke<BranchOption[]>('get_branches', { sessionToken }));
      setBranchAssignment((form) => ({ ...form, branchId: branch.id }));
      setBranchForm({ code: '', nameAr: '', nameEn: '' });
      showNotice(lang === 'ar' ? `تم إنشاء الفرع ${branch.code}.` : `Branch ${branch.code} created.`, 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function assignUserToBranch(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!branchAssignment.branchId || !branchAssignment.userId) { setWorkspaceError(lang === 'ar' ? 'اختر الفرع والمستخدم.' : 'Choose a branch and user.'); return; }
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      await invoke('assign_branch_user', { sessionToken, request: { branchId: branchAssignment.branchId, userId: branchAssignment.userId, roleCode: branchAssignment.roleCode } });
      const [branchRows, userRows] = await Promise.all([
        invoke<BranchOption[]>('get_branches', { sessionToken }), invoke<UserOption[]>('list_users', { sessionToken }),
      ]);
      setBranches(branchRows); setUsers(userRows);
      showNotice(lang === 'ar' ? 'تم إسناد المستخدم إلى الفرع.' : 'User assigned to branch.', 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function changeBranch(branchId: string) {
    if (!branchId || branchId === activeBranchId) return;
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      const result = await invoke<AuthResult>('switch_branch', { sessionToken, branchId });
      setActiveBranchId(result.branchId); setDesktopName(result.displayName); setCart([]); setSelectedCustomer(null);
      setSalesSummary(null); setProfitSummary(null); setInventoryRows([]); setPurchaseOrders([]);
      await loadNativeCatalog(result.sessionToken);
      if (activeTab !== 'sale') await openWorkspaceTab(activeTab);
      showNotice(lang === 'ar' ? 'تم تغيير الفرع النشط.' : 'Active branch changed.', 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function createBackup(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isTauri) { setWorkspaceError(lang === 'ar' ? 'النسخ الاحتياطي المشفّر يتطلب تطبيق سطح المكتب.' : 'Encrypted backups require the desktop app.'); return; }
    if (backupForm.password.length < 12) { setWorkspaceError(lang === 'ar' ? 'استخدم كلمة مرور للنسخة لا تقل عن 12 حرفًا.' : 'Use a backup password of at least 12 characters.'); return; }
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      const result = await invoke<BackupOperationResult>('create_encrypted_backup', { sessionToken, destinationPath: backupForm.destinationPath.trim(), password: backupForm.password, destinationKind: backupForm.destinationKind });
      setBackupHistory(await invoke<BackupHistoryRow[]>('get_backup_history', { sessionToken }));
      setBackupForm((form) => ({ ...form, password: '' }));
      showNotice(lang === 'ar' ? `اكتمل النسخ المشفّر (${(result.byteSize / 1024 / 1024).toFixed(1)} م.ب).` : `Encrypted backup complete (${(result.byteSize / 1024 / 1024).toFixed(1)} MB).`, 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function restoreBackup(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isTauri) { setWorkspaceError(lang === 'ar' ? 'الاستعادة تتطلب تطبيق سطح المكتب.' : 'Restore requires the desktop app.'); return; }
    if (!window.confirm(lang === 'ar' ? 'سيستبدل هذا بيانات الفرع الحالية. سيُنشأ ملف حماية قبل الاستعادة. هل تريد المتابعة؟' : 'This replaces the current store data. A pre-restore backup will be created. Continue?')) return;
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      const result = await invoke<BackupOperationResult>('restore_encrypted_backup', { sessionToken, sourcePath: restoreForm.sourcePath.trim(), password: restoreForm.password });
      setRestoreForm((form) => ({ ...form, password: '' })); setCart([]); setSelectedCustomer(null);
      await Promise.all([loadNativeCatalog(sessionToken), loadBranches(sessionToken)]);
      setBackupHistory(await invoke<BackupHistoryRow[]>('get_backup_history', { sessionToken }));
      showNotice(result.status === 'RESTORED_AUDIT_WARNING' ? (lang === 'ar' ? 'تمت الاستعادة، لكن تعذر تسجيل حدث التدقيق.' : 'Restore finished, but its audit record could not be saved.') : (lang === 'ar' ? 'تمت الاستعادة بعد إنشاء نسخة حماية مسبقة.' : 'Restore completed after creating a pre-restore backup.'), result.status === 'RESTORED_AUDIT_WARNING' ? 'error' : 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function findReturnInvoice(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isTauri) { setWorkspaceError(lang === 'ar' ? 'البحث عن الفواتير يتطلب تطبيق سطح المكتب.' : 'Invoice lookup requires the desktop app.'); return; }
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      const invoice = await invoke<ReturnableInvoice>('get_returnable_invoice', { sessionToken, invoiceNo: returnInvoiceNo.trim() });
      setReturnableInvoice(invoice); setReturnSelections({}); setReturnReason('');
      if (invoice.lines.some((line) => line.remainingQuantity > 0)) showNotice(lang === 'ar' ? 'تم تحميل بنود الفاتورة القابلة للإرجاع.' : 'Returnable invoice lines loaded.', 'success');
    } catch (error) { setReturnableInvoice(null); setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function postReturn() {
    if (!returnableInvoice || !selectedReturnLines.length || !returnReason.trim() || selectedReturnLines.some(({ line, quantity }) => quantity <= 0 || quantity > line.remainingQuantity)) {
      setWorkspaceError(lang === 'ar' ? 'اختر بندًا بكمية صحيحة وأدخل سبب الإرجاع.' : 'Select a valid quantity and enter a return reason.'); return;
    }
    const lines = selectedReturnLines.map(({ line, selection, quantity }) => ({
      invoiceLineId: line.invoiceLineId, quantity, reason: returnReason.trim(), itemCondition: selection.itemCondition, disposition: selection.disposition,
    }));
    const refunds = returnedValueMinor > 0 ? [{ method: refundMethod, amountMinor: returnedValueMinor }] : [];
    const body = { invoiceNo: returnableInvoice.invoiceNo, reason: returnReason.trim(), lines, refunds };
    const idempotencyKey = stableReturnIdempotencyKey(body);
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      const result = await invoke<{ returnNo: string; refundDueMinor: number }>('record_return', { sessionToken, request: { ...body, idempotencyKey } });
      returnOperationRef.current = { payload: '', key: '' };
      setReturnSelections({}); setReturnReason('');
      setReturnableInvoice(await invoke<ReturnableInvoice>('get_returnable_invoice', { sessionToken, invoiceNo: returnableInvoice.invoiceNo }));
      await loadNativeCatalog(sessionToken);
      showNotice(lang === 'ar' ? `تم تسجيل المرتجع ${result.returnNo} بقيمة ${money(result.refundDueMinor, lang)}.` : `Return ${result.returnNo} recorded for ${money(result.refundDueMinor, lang)}.`, 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function postExchange() {
    const quantity = Math.trunc(Number(exchangeQuantity));
    if (!returnableInvoice || !selectedReturnLines.length || !returnReason.trim() || !exchangeVariant || quantity <= 0
      || selectedReturnLines.some(({ line, quantity: selectedQuantity }) => selectedQuantity <= 0 || selectedQuantity > line.remainingQuantity)) {
      setWorkspaceError(lang === 'ar' ? 'تحقق من الفاتورة والبنود المرتجعة والصنف البديل والكمية.' : 'Check the original invoice, returned lines, replacement item, and quantity.'); return;
    }
    const returnedLines = selectedReturnLines.map(({ line, selection, quantity: selectedQuantity }) => ({
      invoiceLineId: line.invoiceLineId, quantity: selectedQuantity, reason: returnReason.trim(), itemCondition: selection.itemCondition, disposition: selection.disposition,
    }));
    const payments = exchangeDifference > 0 ? [{ method: exchangePaymentMethod, amount_minor: exchangeDifference }] : [];
    const refunds = exchangeDifference < 0 ? [{ method: exchangeRefundMethod, amountMinor: -exchangeDifference }] : [];
    const body = {
      invoiceNo: returnableInvoice.invoiceNo, reason: returnReason.trim(), returnedLines,
      replacementLines: [{ variant_id: exchangeVariant.id, quantity, item_discount_bps: 0 }],
      invoiceDiscountBps: 0, discountReason: null, payments, refunds,
    };
    const idempotencyKey = stableReturnIdempotencyKey(body);
    setWorkspaceBusy(true); setWorkspaceError('');
    try {
      const result = await invoke<{ returnNo: string; replacementInvoiceNo: string; priceDifferenceMinor: number }>('create_exchange', { sessionToken, request: { ...body, idempotencyKey } });
      returnOperationRef.current = { payload: '', key: '' };
      setReturnSelections({}); setReturnReason('');
      setReturnableInvoice(await invoke<ReturnableInvoice>('get_returnable_invoice', { sessionToken, invoiceNo: returnableInvoice.invoiceNo }));
      await loadNativeCatalog(sessionToken);
      const settlement = result.priceDifferenceMinor > 0
        ? (lang === 'ar' ? `تحصيل ${money(result.priceDifferenceMinor, lang)}` : `collect ${money(result.priceDifferenceMinor, lang)}`)
        : result.priceDifferenceMinor < 0
          ? (lang === 'ar' ? `رد ${money(-result.priceDifferenceMinor, lang)}` : `refund ${money(-result.priceDifferenceMinor, lang)}`)
          : (lang === 'ar' ? 'دون فرق سعر' : 'no price difference');
      showNotice(lang === 'ar' ? `تم الاستبدال ${result.returnNo} · ${result.replacementInvoiceNo} · ${settlement}.` : `Exchange ${result.returnNo} · ${result.replacementInvoiceNo} · ${settlement}.`, 'success');
    } catch (error) { setWorkspaceError(String(error)); }
    finally { setWorkspaceBusy(false); }
  }

  async function loadReports() {
    if (!isTauri) { setWorkspaceError(lang === 'ar' ? 'التقارير الحية تتطلب تطبيق سطح المكتب.' : 'Live reports require the desktop app.'); return; }
    const fromDate = new Date(`${reportRange.from}T00:00:00`);
    const toDate = new Date(`${reportRange.to}T00:00:00`);
    if (Number.isNaN(fromDate.getTime()) || Number.isNaN(toDate.getTime()) || fromDate > toDate) {
      setWorkspaceError(lang === 'ar' ? 'نطاق التاريخ غير صالح.' : 'Invalid date range.'); return;
    }
    toDate.setDate(toDate.getDate() + 1);
    setWorkspaceBusy(true);
    setWorkspaceError('');
    try {
      const range = { sessionToken, fromUtc: fromDate.toISOString(), toUtc: toDate.toISOString() };
      const summary = await invoke<SalesSummary>('get_sales_summary', range);
      setSalesSummary(summary);
      try { setProfitSummary(await invoke<ProfitSummary>('get_profit_summary', range)); }
      catch { setProfitSummary(null); }
    } catch (error) {
      setWorkspaceError(String(error));
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function submitAuth(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setAuthBusy(true);
    setAuthError('');
    try {
      const result = gate === 'setup'
        ? await invoke<AuthResult>('bootstrap_owner', { username: authForm.username, displayName: authForm.displayName, password: authForm.password })
        : await invoke<AuthResult>('login', { username: authForm.username, password: authForm.password });
      setSessionToken(result.sessionToken);
      setDesktopName(result.displayName);
      setActiveBranchId(result.branchId);
      try { setBranches(await invoke<BranchOption[]>('get_branches', { sessionToken: result.sessionToken })); } catch { setBranches([]); }
      // The tax is optional, so first run goes straight to the till; the
      // owner turns it on from the control panel when the store needs it.
      setGate('ready');
      await loadNativeCatalog(result.sessionToken);
      setAuthForm((form) => ({ ...form, password: '' }));
    } catch {
      setAuthError(lang === 'ar' ? 'تعذر تسجيل الدخول أو إعداد الحساب. راجع البيانات وحاول مرة أخرى.' : 'Could not sign in or create the owner. Check the details and try again.');
    } finally {
      setAuthBusy(false);
    }
  }

  async function saveTaxConfiguration(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const typed = Math.round(Number(taxRateInput.replace(',', '.')) * 100);
    const rateBps = taxEnabledInput ? typed : 0;
    if (!Number.isFinite(typed) || typed < 0 || typed > 10000) {
      setAuthError(lang === 'ar' ? 'نسبة ضريبة غير صالحة.' : 'Invalid tax rate.');
      return;
    }
    setAuthBusy(true);
    setAuthError('');
    try {
      await invoke('configure_tax', { sessionToken, taxRateBps: rateBps, pricesIncludeTax: taxInclusiveInput });
      setTaxRateBps(rateBps);
      setTaxEnabled(rateBps > 0);
      setPricesIncludeTax(taxInclusiveInput);
      setTaxNeedsConfiguration(false);
      setGate('ready');
      await loadNativeCatalog(sessionToken);
    } catch {
      setAuthError(lang === 'ar' ? 'تعذر حفظ إعداد الضريبة.' : 'Could not save tax settings.');
    } finally {
      setAuthBusy(false);
    }
  }

  const filteredProducts = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase(lang === 'ar' ? 'ar' : 'en');
    return catalogItems.filter((product) => {
      const categoryMatch = category === 'all' || product.category === category;
      const text = [product.nameAr, product.nameEn, product.brand, ...product.variants.flatMap((v) => [v.sku, v.barcode])].join(' ').toLocaleLowerCase();
      return categoryMatch && (!normalized || text.includes(normalized));
    });
  }, [category, query, lang, catalogItems]);

  function showNotice(text: string, kind: 'success' | 'error' = 'error') {
    setNotice({ text, kind });
  }

  function openVariantPicker(product: Product) {
    setActiveProduct(product);
    setSelectedSize(product.variants[0]?.size ?? '');
    setSelectedColor(product.variants[0]?.color ?? '');
  }

  function addVariant(product: Product, variant: Variant) {
    if (variant.stock < 1) {
      showNotice(t.outOfStock);
      return;
    }
    setCart((current) => {
      const found = current.find((line) => line.variant.id === variant.id);
      if (found && found.quantity >= variant.stock) {
        showNotice(t.outOfStock);
        return current;
      }
      if (found) return current.map((line) => line.variant.id === variant.id ? { ...line, quantity: line.quantity + 1 } : line);
      return [...current, { product, variant, quantity: 1, discountBps: 0 }];
    });
    setActiveProduct(null);
    setQuery('');
  }

  function updateQuantity(variantId: string, delta: number) {
    setCart((current) => current.flatMap((line) => {
      if (line.variant.id !== variantId) return [line];
      const next = line.quantity + delta;
      if (next <= 0) return [];
      if (next > line.variant.stock) {
        showNotice(t.outOfStock);
        return [line];
      }
      return [{ ...line, quantity: next }];
    }));
  }

  function changePayment(id: string, value: string) {
    const parsed = Number(value.replace(',', '.'));
    const minor = Number.isFinite(parsed) ? Math.max(0, Math.round(parsed * 100)) : 0;
    setPayments((current) => current.map((row) => row.id === id ? { ...row, amountMinor: minor } : row));
  }

  function addPaymentLine() {
    const nextMethod: PayMethod = payments.some((p) => p.method === 'CARD') ? 'WALLET' : 'CARD';
    const next = Math.max(0, totals.total - paid);
    setPayments((current) => [...current, { id: `pay-${Date.now()}`, method: nextMethod, amountMinor: next }]);
  }

  async function completeSale() {
    if (!cart.length) {
      showNotice(t.needItems);
      return;
    }
    if (paid !== totals.total) {
      showNotice(t.needPayment);
      return;
    }
    if (manualDiscountUsed && !discountReason.trim()) {
      showNotice(t.reasonRequired);
      return;
    }

    try {
      let invoiceNo = '';
      if (isTauri) {
        // Production command rechecks session, price, tax, stock and permissions in Rust.
        const result = await invoke<{ invoiceNo: string; invoiceId?: string }>('create_sale', {
          sessionToken,
          request: {
            idempotency_key: crypto.randomUUID(),
            customer_id: selectedCustomer?.id ?? null,
            lines: cart.map((line) => ({ variant_id: line.variant.id, quantity: line.quantity, item_discount_bps: line.discountBps })),
            invoice_discount_bps: invoiceDiscountBps,
            discount_reason: discountReason.trim() || null,
            payments: payments.map((payment) => ({ method: payment.method, amount_minor: payment.amountMinor })),
          },
        });
        invoiceNo = result.invoiceNo;
        setLastInvoiceId(result.invoiceId ?? null);
        await loadNativeCatalog(sessionToken);
      } else {
        // Browser preview intentionally demonstrates only the interaction flow; it does not persist a real sale.
        invoiceNo = `NS-${new Date().toISOString().slice(0, 10).replaceAll('-', '')}-${String(invoiceCount + 1).padStart(4, '0')}`;
        setInvoiceCount((count) => count + 1);
        setSalesToday((amount) => amount + totals.total);
      }
      setLastInvoice(invoiceNo);
      setCart([]);
      setInvoiceDiscountBps(0);
      setDiscountReason('');
      setPayments([{ id: `pay-${Date.now()}`, method: 'CASH', amountMinor: 0 }]);
      showNotice(`${t.saleDone} · ${invoiceNo}`, 'success');
    } catch {
      showNotice('تعذر إتمام البيع. لم يتم تغيير المخزون؛ أعد المحاولة أو اتصل بالمدير.');
    }
  }

  async function printLastInvoice() {
    if (!lastInvoiceId) {
      showNotice(t.needItems, 'error');
      return;
    }
    if (!isTauri) {
      showNotice(lang === 'ar' ? 'الطباعة متاحة في تطبيق سطح المكتب فقط.' : 'Printing is available in the desktop app only.', 'error');
      return;
    }
    try {
      const outcome = await invoke<{ printed: boolean; mode: string; printer: string | null; detail: string }>('print_invoice', {
        sessionToken,
        invoiceId: lastInvoiceId,
        mode: 'direct',
        paper: '58',
      });
      if (outcome.printed) {
        showNotice(lang === 'ar' ? `تم إرسال الفاتورة إلى ${outcome.printer ?? 'الطابعة'}.` : `Receipt sent to ${outcome.printer ?? 'the printer'}.`, 'success');
      } else {
        showNotice(t.printFailed, 'error');
      }
    } catch (error) {
      // Fall back to the print dialog, which works with drivers that cannot
      // accept a raw byte stream.
      try {
        await invoke('print_invoice', {
          sessionToken,
          invoiceId: lastInvoiceId,
          mode: 'dialog',
          paper: '58',
        });
        showNotice(lang === 'ar' ? 'تم فتح نافذة الطباعة.' : 'Print dialog opened.', 'success');
      } catch {
        showNotice(`${t.printFailed} (${String(error)})`, 'error');
      }
    }
  }

  const categoryEntries = Object.keys(categoryLabels) as CategoryKey[];
  const itemCount = cart.reduce((sum, line) => sum + line.quantity, 0);

  if (isTauri && gate !== 'ready') {
    const checking = gate === 'checking';
    const licenseGate = gate === 'license';
    const taxSetup = gate === 'tax';
    const setup = gate === 'setup';
    return (
      <>
      <div className="auth-shell" dir={dir}>
        <div className="auth-brand"><div className={`auth-logo ${logoPress.isHolding ? 'holding' : ''}`} title={lang === 'ar' ? 'اضغط مطولًا للوحة المالك' : 'Long press for the owner panel'} {...logoPress.handlers}>ن</div><div><strong>نسيج</strong><span>{lang === 'ar' ? 'نظام نقاط البيع' : 'Point of sale'}</span></div></div>
        <section className="auth-card">
          <div className="auth-card-mark">{licenseGate || taxSetup ? <Icon name="tag" size={22} /> : <Icon name={checking ? 'clock' : 'user'} size={22} />}</div>
          <div className="auth-eyebrow">{lang === 'ar' ? 'تشغيل محلي آمن' : 'Secure local setup'}</div>
          <h1>{checking ? (lang === 'ar' ? 'جارٍ التحقق…' : 'Checking…') : licenseGate ? (lang === 'ar' ? 'تفعيل هذا الجهاز' : 'Activate this device') : taxSetup ? (lang === 'ar' ? 'إعداد الضريبة' : 'Tax setup') : setup ? (lang === 'ar' ? 'إنشاء حساب المالك' : 'Create owner account') : gate === 'error' ? (lang === 'ar' ? 'تعذر فتح قاعدة البيانات' : 'Could not open the database') : (lang === 'ar' ? 'تسجيل الدخول' : 'Sign in')}</h1>
          <p>{checking ? (lang === 'ar' ? 'يتم التحقق من الترخيص وقاعدة البيانات المحلية المشفّرة.' : 'Checking the license and opening the encrypted local database.') : licenseGate ? (lang === 'ar' ? 'صدّر طلب التفعيل وانقله إلى أداة الإصدار المنفصلة، ثم استورد ملف الترخيص الموقّع. لا يلزم اتصال بالإنترنت.' : 'Export an activation request for the separate issuer tool, then import its signed license file. No internet connection is required.') : taxSetup ? (lang === 'ar' ? 'اختر قاعدة الضريبة قبل أول عملية بيع. يمكنك تعديلها لاحقًا بصلاحية المالك.' : 'Choose the tax mode before the first sale. The owner can change it later.') : setup ? (lang === 'ar' ? 'هذا الحساب الأول بصلاحية المالك. لا توجد كلمة مرور افتراضية.' : 'This first account has owner privileges. No default password is used.') : gate === 'error' ? (lang === 'ar' ? 'لم يتم إنشاء قاعدة بديلة غير مشفّرة. راجع الدعم الفني.' : 'No plaintext fallback database was created. Contact support.') : (lang === 'ar' ? 'أدخل بيانات حسابك للمتابعة إلى نقطة البيع.' : 'Enter your account details to continue.')}</p>
          {licenseGate ? <div className="auth-form">
            <div className="auth-warning">{lang === 'ar' ? `حالة الترخيص: ${licenseStatus?.statusCode ?? 'غير متاح'}.` : `License status: ${licenseStatus?.statusCode ?? 'unavailable'}.`} {!licenseStatus?.publicKeyConfigured && (lang === 'ar' ? ' مفتاح التحقق العام غير مضبوط في نسخة التطبيق؛ لا يمكن قبول التراخيص قبل إعداد إصدار موقّع.' : ' The verification public key is not configured in this build; signed licenses cannot be accepted until a release key is set.')}</div>
            {licenseStatus?.deviceFingerprint && <div className="auth-helper mono">{lang === 'ar' ? 'بصمة الجهاز: ' : 'Device fingerprint: '}{licenseStatus.deviceFingerprint}</div>}
            {licenseError && <div className="auth-error">{licenseError}</div>}
            <button className="auth-submit" type="button" onClick={exportActivationRequest} disabled={!licenseStatus?.deviceFingerprint || !licenseStatus?.publicKeyConfigured}>{lang === 'ar' ? 'تصدير طلب التفعيل' : 'Export activation request'}<Icon name="chevron" size={16} /></button>
            <label>{lang === 'ar' ? 'استيراد ملف الترخيص الموقّع (.lic أو .json)' : 'Import signed license file (.lic or .json)'}<input type="file" accept=".lic,.json,application/json" onChange={importLicenseFile} disabled={authBusy || !licenseStatus?.publicKeyConfigured} /></label>
            {authBusy && <div className="auth-loader"><span />{lang === 'ar' ? 'جارٍ التحقق من التوقيع…' : 'Verifying signature…'}</div>}
          </div> : taxSetup ? <form onSubmit={saveTaxConfiguration} className="auth-form">
            <label>{lang === 'ar' ? 'نسبة الضريبة (%)' : 'Tax rate (%)'}<input type="number" min="0" max="100" step="0.01" value={taxRateInput} onChange={(event) => setTaxRateInput(event.target.value)} required /></label>
            <div className="tax-choice"><button type="button" className={taxInclusiveInput ? 'chosen' : ''} onClick={() => setTaxInclusiveInput(true)}>{lang === 'ar' ? 'السعر شامل الضريبة' : 'Prices include tax'}</button><button type="button" className={!taxInclusiveInput ? 'chosen' : ''} onClick={() => setTaxInclusiveInput(false)}>{lang === 'ar' ? 'السعر غير شامل' : 'Prices exclude tax'}</button></div>
            <div className="auth-warning">{lang === 'ar' ? 'الافتراضي 14% حسب بيانات الإعداد؛ أكّد النسبة وطريقة العرض مع محاسب المتجر.' : 'The prefilled 14% must be confirmed with your store accountant.'}</div>
            {authError && <div className="auth-error">{authError}</div>}
            <button className="auth-submit" type="submit" disabled={authBusy}>{authBusy ? '…' : (lang === 'ar' ? 'حفظ ومتابعة' : 'Save and continue')}<Icon name="chevron" size={16} /></button>
          </form> : gate === 'error' ? <button className="auth-submit" onClick={() => window.location.reload()}>{lang === 'ar' ? 'إعادة المحاولة' : 'Retry'}</button> : checking ? <div className="auth-loader"><span />{lang === 'ar' ? 'لحظة واحدة' : 'One moment'}</div> : <form onSubmit={submitAuth} className="auth-form">
            {setup && <label>{lang === 'ar' ? 'الاسم الظاهر' : 'Display name'}<input value={authForm.displayName} onChange={(event) => setAuthForm((form) => ({ ...form, displayName: event.target.value }))} autoComplete="name" required /></label>}
            <label>{lang === 'ar' ? 'اسم المستخدم' : 'Username'}<input value={authForm.username} onChange={(event) => setAuthForm((form) => ({ ...form, username: event.target.value }))} autoComplete="username" required /></label>
            <label>{lang === 'ar' ? 'كلمة المرور' : 'Password'}<input type="password" value={authForm.password} onChange={(event) => setAuthForm((form) => ({ ...form, password: event.target.value }))} autoComplete={setup ? 'new-password' : 'current-password'} minLength={setup ? 12 : 1} required /></label>
            {setup && <small className="auth-helper">{lang === 'ar' ? 'استخدم 12 حرفًا على الأقل؛ خزّنت بصيغة Argon2id.' : 'Use at least 12 characters; stored as Argon2id.'}</small>}
            {authError && <div className="auth-error">{authError}</div>}
            <button className="auth-submit" type="submit" disabled={authBusy}>{authBusy ? '…' : setup ? (lang === 'ar' ? 'إنشاء المالك' : 'Create owner') : (lang === 'ar' ? 'دخول' : 'Sign in')}<Icon name="chevron" size={16} /></button>
          </form>}
          <div className="auth-foot"><span className="status-dot" />{lang === 'ar' ? 'البيانات محفوظة محليًا ومشفّرة' : 'Data is encrypted and stored locally'}</div>
        </section>
        <div className="auth-top-actions">
          <button className="auth-language" onClick={() => setLang(lang === 'ar' ? 'en' : 'ar')}>{lang === 'ar' ? 'English' : 'العربية'}</button>
          <button className="auth-theme-button" onClick={toggleTheme} aria-label={lang === 'ar' ? 'تبديل النمط' : 'Toggle theme'}><Icon name={theme === 'dark' ? 'sun' : 'moon'} size={14} /></button>
        </div>
      </div>
      {ownerPanelOpen && <OwnerPanel lang={lang} onClose={() => setOwnerPanelOpen(false)} />}
      </>
    );
  }

  return (
    <div className={`app-shell ${managerMode ? 'manager-mode' : ''}`} dir={dir}>
      <aside className="side-rail manager-sidebar">
        <button
          type="button"
          className="manager-brand"
          title={lang === 'ar' ? 'اضغط مطولًا للوحة المالك' : 'Long press for the owner panel'}
          {...logoPress.handlers}
        >
          <span className="brand-hanger"><Icon name="hanger" size={24} /></span>
          <span className="brand-copy"><strong>Fashion Store</strong><small>STORE MANAGEMENT</small></span>
        </button>

        <div className="sidebar-scroll">
          <button
            type="button"
            className={`nav-button ${managerMode && managerSection === 'overview' ? 'active' : ''}`}
            onClick={() => { setActiveTab('manager'); setManagerSection('overview'); }}
          ><Icon name="home" size={17} />{lang === 'ar' ? 'نظرة عامة' : 'Overview'}</button>
          <button
            type="button"
            className={`nav-button ${managerMode && managerSection === 'orders' ? 'active' : ''}`}
            onClick={() => { setActiveTab('manager'); setManagerSection('orders'); }}
          ><Icon name="cart" size={17} />{lang === 'ar' ? 'الطلبات' : 'Orders'}<span className="nav-badge">1</span></button>
          <button type="button" className={`nav-button ${activeTab === 'products' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('products')}><Icon name="boxes" size={17} />{lang === 'ar' ? 'المنتجات والمخزون' : 'Products & inventory'}</button>
          <button type="button" className={`nav-button ${activeTab === 'inventory' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('inventory')}><Icon name="box" size={17} />{t.inventory}</button>
          <button type="button" className={`nav-button ${activeTab === 'purchases' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('purchases')}><Icon name="wallet" size={17} />{t.purchases}</button>
          <button type="button" className={`nav-button ${activeTab === 'returns' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('returns')}><Icon name="return" size={17} />{t.returns}</button>
          <button type="button" className={`nav-button ${activeTab === 'users' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('users')}><Icon name="users" size={17} />{lang === 'ar' ? 'العملاء والحسابات' : 'Customers & accounts'}</button>
          <button
            type="button"
            className={`nav-button ${managerMode && managerSection === 'support' ? 'active' : ''}`}
            onClick={() => { setActiveTab('manager'); setManagerSection('support'); }}
          ><Icon name="message" size={17} />{lang === 'ar' ? 'رسائل الدعم' : 'Support messages'}<span className="nav-badge">3</span></button>
          <button type="button" className={`nav-button ${activeTab === 'products' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('products')}><Icon name="ruler" size={17} />{lang === 'ar' ? 'خيارات الحجم' : 'Size options'}</button>
          <button type="button" className={`nav-button ${activeTab === 'reports' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('reports')}><Icon name="chart" size={17} />{t.reports}</button>

          <div className="nav-divider" />

          <button type="button" className={`nav-button ${activeTab === 'branches' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('branches')}><Icon name="settings" size={17} />{lang === 'ar' ? 'إدارة المتجر' : 'Store settings'}</button>
          <button
            type="button"
            className={`nav-button ${managerMode && managerSection === 'activity' ? 'active' : ''}`}
            onClick={() => { setActiveTab('manager'); setManagerSection('activity'); }}
          ><Icon name="activity" size={17} />{lang === 'ar' ? 'سجل النشاط' : 'Activity log'}</button>
          <button
            type="button"
            className={`nav-button ${managerMode && managerSection === 'support' ? 'active' : ''}`}
            onClick={() => { setActiveTab('manager'); setManagerSection('support'); }}
          ><Icon name="inbox" size={17} />{lang === 'ar' ? 'صندوق الرسائل المحلي' : 'Local message inbox'}</button>
          <button type="button" className={`nav-button ${activeTab === 'imports' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('imports')}><Icon name="boxes" size={17} />{lang === 'ar' ? 'استيراد البيانات' : 'Data import'}</button>
          <button type="button" className={`nav-button ${activeTab === 'backup' ? 'active' : ''}`} onClick={() => void openWorkspaceTab('backup')}><Icon name="archive" size={17} />{t.backup}</button>
        </div>

        <div className="sidebar-bottom">
          <div className="sidebar-tools">
            <button type="button" className="sidebar-tool" onClick={() => setLang(lang === 'ar' ? 'en' : 'ar')}>{lang === 'ar' ? 'EN' : 'ع'}</button>
            <button type="button" className="sidebar-tool" onClick={toggleTheme} aria-label={lang === 'ar' ? 'تبديل النمط' : 'Toggle theme'}><Icon name={theme === 'dark' ? 'sun' : 'moon'} size={14} /></button>
            <div className="status-chip"><span className="status-dot" />{lang === 'ar' ? 'محلي' : 'Local'}</div>
          </div>
          <button type="button" className="store-view-button" onClick={() => void openWorkspaceTab('sale')}><Icon name="home" size={16} />{lang === 'ar' ? 'عرض المتجر' : 'Open store'}</button>
          <span className="sidebar-copyright">Fashion Store © 2026</span>
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar manager-topbar">
          <div className="manager-top-actions">
            <button type="button" className="top-icon-button" title={lang === 'ar' ? 'تسجيل الخروج' : 'Sign out'} onClick={() => setNotice({ text: lang === 'ar' ? 'تسجيل الخروج متاح من نافذة التطبيق.' : 'Sign out is available from the app window.', kind: 'success' })}><Icon name="logout" size={17} /></button>
            <div className="top-location"><span className="location-avatar">م</span><span>{lang === 'ar' ? 'مصر المعتمدية' : 'El Maamouria, Egypt'}</span></div>
            <button type="button" className="top-icon-button" title={lang === 'ar' ? 'الإشعارات' : 'Notifications'}><Icon name="bell" size={17} /></button>
            <label className="top-search"><Icon name="search" size={16} /><input placeholder={lang === 'ar' ? 'ابحث عن رقم أو اسم عميل...' : 'Search by customer name or number...'} aria-label={lang === 'ar' ? 'بحث' : 'Search'} /></label>
            <div className="manager-top-account"><span className="account-avatar">{(desktopName || 'P').slice(0, 1)}</span><span>{desktopName || (lang === 'ar' ? 'مدير المتجر' : 'Store manager')}</span><Icon name="chevronDown" size={12} /></div>
            {branches.length > 0 && <select className="branch-switcher manager-branch-switcher" value={activeBranchId} onChange={(event) => void changeBranch(event.target.value)} aria-label={lang === 'ar' ? 'الفرع النشط' : 'Active branch'}>{branches.map((branch) => <option key={branch.id} value={branch.id}>{branch.code} · {lang === 'ar' ? branch.nameAr : branch.nameEn}</option>)}</select>}
          </div>
          <div className="topbar-center"><span className="breadcrumb-muted">{lang === 'ar' ? 'الإدارة' : 'Management'}</span><span className="breadcrumb-sep"><Icon name="chevron" size={13} /></span><strong>{sectionTitle}</strong></div>
        </header>

        {licenseStatus && !licenseStatus.productionBuild && (
          <div className="license-banner license-banner-danger" role="alert">
            <Icon name="warning" size={16} />
            <span>
              {lang === 'ar'
                ? 'نسخة تطوير بدون مفتاح ترخيص إنتاجي — لا تُوزَّع على العملاء.'
                : 'Development build without a production licence key — do not distribute to customers.'}
            </span>
          </div>
        )}

        {licenseStatus && (licenseStatus.localTrial || licenseStatus.gracePeriod) && (
          <div className="license-banner" role="status">
            <Icon name="clock" size={16} />
            <span>
              {(() => {
                const dateLocale = lang === 'ar' ? 'ar-EG' : 'en-GB';
                const graceEnd = licenseStatus.graceExpiresAtUtc
                  ? new Date(licenseStatus.graceExpiresAtUtc).toLocaleDateString(dateLocale)
                  : '—';
                const trialEnd = licenseStatus.expiresAtUtc
                  ? new Date(licenseStatus.expiresAtUtc).toLocaleDateString(dateLocale)
                  : '—';
                if (licenseStatus.gracePeriod) {
                  return lang === 'ar'
                    ? `انتهت صلاحية الترخيص — مهلة سماح حتى ${graceEnd}. فعّل اشتراكًا للاستمرار.`
                    : `Licence expired — grace period until ${graceEnd}. Activate a subscription to continue.`;
                }
                return lang === 'ar'
                  ? `نسخة تجريبية محلية — متاحة حتى ${trialEnd}.`
                  : `Local evaluation — available until ${trialEnd}.`;
              })()}
            </span>
          </div>
        )}

        <div className="page-content">
          {managerMode ? (
            <StoreDashboard
              section={managerSection}
              onSectionChange={setManagerSection}
              onNavigate={(tab: PosTarget) => void openWorkspaceTab(tab)}
            />
          ) : activeTab === 'sale' ? <>
          <div className="page-heading">
            <div><div className="eyebrow"><span className="live-dot" />{t.scanTip}</div><h1>{t.title}</h1><p>{t.subtitle}</p></div>
            <div className="date-block"><div className="date-icon"><Icon name="clock" size={17} /></div><div><span>{new Intl.DateTimeFormat(lang === 'ar' ? 'ar-EG' : 'en-GB', { weekday: 'long', day: 'numeric', month: 'long' }).format(new Date())}</span><strong>{new Intl.DateTimeFormat(lang === 'ar' ? 'ar-EG' : 'en-GB', { hour: '2-digit', minute: '2-digit' }).format(new Date())}</strong></div></div>
          </div>

          <section className="stats-row" aria-label="ملخص اليوم">
            <div className="stat-card"><div className="stat-icon mint"><Icon name="bag" size={17} /></div><div className="stat-copy"><span>{t.today}</span><strong>{money(salesToday, lang)}</strong></div><span className="stat-trend">+12.8%</span></div>
            <div className="stat-card"><div className="stat-icon lilac"><Icon name="printer" size={17} /></div><div className="stat-copy"><span>{t.invoices}</span><strong>{invoiceCount}</strong></div><span className="stat-note">{lang === 'ar' ? 'منذ ٩ صباحًا' : 'since 9:00 AM'}</span></div>
            <div className="stat-card"><div className="stat-icon peach"><Icon name="tag" size={17} /></div><div className="stat-copy"><span>{t.average}</span><strong>{money(Math.round(salesToday / Math.max(invoiceCount, 1)), lang)}</strong></div><span className="stat-note">{lang === 'ar' ? 'هذا اليوم' : 'today'}</span></div>
          </section>

          <div className="workspace-grid">
            <section className="catalog-panel">
              <div className="catalog-toolbar">
                <div className="search-box"><Icon name="search" size={18} /><input ref={searchRef} value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t.search} aria-label={t.search} /><kbd>F2</kbd><span className="search-scan"><Icon name="scan" size={18} /></span></div>
                <button className="filter-button"><span className="filter-symbol">≡</span>{t.selectAll}</button>
              </div>

              <div className="category-row" aria-label="التصنيفات">
                {categoryEntries.map((key) => <button key={key} className={`category-chip ${category === key ? 'selected' : ''}`} onClick={() => setCategory(key)}>{t[categoryLabels[key]]}<span>{key === 'all' ? catalogItems.length : catalogItems.filter((p) => p.category === key).length}</span></button>)}
              </div>

              <div className="section-line"><div><h2>{lang === 'ar' ? 'المنتجات' : 'Products'}</h2><span>{filteredProducts.length} {lang === 'ar' ? 'صنف' : 'items'}</span></div><button className="view-all">{lang === 'ar' ? 'الأكثر مبيعًا' : 'Best selling'}<Icon name="chevronDown" size={14} /></button></div>

              <div className="product-grid">
                {filteredProducts.map((product) => {
                  const stock = product.variants.reduce((sum, variant) => sum + variant.stock, 0);
                  const badgeLabel = product.badge ? t[product.badge] : null;
                  return (
                    <button type="button" className="product-card" key={product.id} onClick={() => openVariantPicker(product)}>
                      <div className={`product-art ${product.art}`}>
                        {badgeLabel && <span className={`product-badge ${product.badge}`}>{badgeLabel}</span>}
                        <div className="garment-shape"><span /></div>
                        <div className="product-colors">{[...new Set(product.variants.map((v) => v.color))].slice(0, 4).map((colorKey) => <i key={colorKey} style={{ backgroundColor: colors.find((c) => c.key === colorKey)?.hex }} />)}</div>
                      </div>
                      <div className="product-details"><div className="product-title-row"><h3>{lang === 'ar' ? product.nameAr : product.nameEn}</h3><span className="product-stock">{stock} {t.units}</span></div><div className="product-meta"><span>{product.brand}</span><span>{lang === 'ar' ? product.categoryAr : product.categoryEn}</span></div><div className="product-price-row"><strong>{money(product.priceMinor, lang)}</strong><span className="quick-add"><Icon name="plus" size={15} /></span></div></div>
                    </button>
                  );
                })}
                {filteredProducts.length === 0 && <div className="no-results"><div className="no-results-icon"><Icon name="search" size={22} /></div><strong>{t.searchEmpty}</strong><span>{lang === 'ar' ? 'جرّب كلمة بحث أخرى أو غيّر التصنيف' : 'Try another search or category'}</span></div>}
              </div>
              <div className="catalog-footnote"><span className="footnote-dot" />{lang === 'ar' ? 'الرصيد المعروض حسب المقاس واللون' : 'Stock shown per size and color'}<span className="catalog-count">{t.searchHint} · {filteredProducts.length} {t.items}</span></div>
            </section>

            <aside className="cart-panel">
              <div className="cart-heading"><div><div className="cart-title-line"><h2>{t.cart}</h2><span className="cart-count">{itemCount}</span></div><p>{lastInvoice === '—' ? (lang === 'ar' ? 'فاتورة جديدة' : 'New invoice') : `${lang === 'ar' ? 'آخر فاتورة' : 'Last receipt'}: ${lastInvoice}`}</p></div><button className="more-button" aria-label="المزيد">•••</button></div>
              <div className="customer-strip"><div className="customer-avatar"><Icon name="user" size={17} /></div><div><strong>{selectedCustomer?.name ?? t.customer}</strong><small>{selectedCustomer?.phone ?? (lang === 'ar' ? 'احفظ سجل المشتريات والنقاط' : 'Purchase history and loyalty')}</small></div>{selectedCustomer ? <button className="add-customer" onClick={() => setSelectedCustomer(null)}><Icon name="close" size={15} />{lang === 'ar' ? 'إزالة' : 'Remove'}</button> : <button className="add-customer" onClick={() => { setCustomerMessage(''); setCustomerModalOpen(true); }}><Icon name="plus" size={15} />{t.addCustomer}</button>}</div>

              <div className={`cart-lines ${cart.length === 0 ? 'is-empty' : ''}`}>
                {cart.length === 0 ? <div className="empty-state"><div className="empty-bag"><Icon name="bag" size={24} /></div><strong>{t.empty}</strong><span>{t.emptyHint}</span></div> : cart.map((line) => <div className="cart-line" key={line.variant.id}>
                  <div className={`line-thumb ${line.product.art}`}><div className="mini-garment" /></div>
                  <div className="line-body"><div className="line-title-row"><div><strong>{lang === 'ar' ? line.product.nameAr : line.product.nameEn}</strong><span>{line.variant.size} · {lang === 'ar' ? line.variant.colorAr : line.variant.colorEn}</span></div><button className="remove-line" onClick={() => setCart((current) => current.filter((item) => item.variant.id !== line.variant.id))} aria-label="حذف"><Icon name="trash" size={15} /></button></div>
                    <div className="line-price-row"><span>{money(line.variant.priceMinor, lang)}</span><strong>{money(line.variant.priceMinor * line.quantity * (10000 - line.discountBps) / 10000, lang)}</strong></div>
                    <div className="line-controls"><div className="qty-stepper"><button onClick={() => updateQuantity(line.variant.id, -1)} aria-label="تقليل"><Icon name="minus" size={13} /></button><span>{line.quantity}</span><button onClick={() => updateQuantity(line.variant.id, 1)} aria-label="زيادة"><Icon name="plus" size={13} /></button></div><button className="line-discount" onClick={() => setCart((current) => current.map((item) => item.variant.id === line.variant.id ? { ...item, discountBps: item.discountBps === 0 ? 500 : 0 } : item))}><Icon name="tag" size={13} />{line.discountBps ? `${line.discountBps / 100}%` : t.discount}</button></div>
                  </div>
                </div>)}
              </div>

              <div className="discount-control"><div><span className="discount-icon"><Icon name="tag" size={15} /></span><strong>{t.invoiceDiscount}</strong></div><label className="percent-input"><input type="number" min="0" max="100" step="1" value={invoiceDiscountBps / 100} onChange={(event) => setInvoiceDiscountBps(Math.min(10000, Math.max(0, Number(event.target.value) * 100 || 0)))} aria-label={t.invoiceDiscount} /><span>%</span></label></div>
              {manualDiscountUsed && <input className="discount-reason-input" value={discountReason} onChange={(event) => setDiscountReason(event.target.value)} placeholder={t.discountReason} />}

              <div className="payment-block"><div className="payment-heading"><strong>{t.payment}</strong><button onClick={addPaymentLine}><Icon name="plus" size={14} />{t.addPayment}</button></div>
                {payments.map((payment) => <div className="payment-row" key={payment.id}><div className={`payment-method-icon ${payment.method.toLowerCase()}`}><Icon name={payment.method === 'CASH' ? 'wallet' : payment.method === 'CARD' ? 'credit' : 'bag'} size={16} /></div><select value={payment.method} onChange={(event) => setPayments((current) => current.map((row) => row.id === payment.id ? { ...row, method: event.target.value as PayMethod } : row))} aria-label={t.payment}><option value="CASH">{t.cash}</option><option value="CARD">{t.card}</option><option value="WALLET">{t.wallet}</option></select><label className="payment-amount"><input type="number" min="0" step="1" value={payment.amountMinor ? payment.amountMinor / 100 : ''} onChange={(event) => changePayment(payment.id, event.target.value)} placeholder="0" /><span>{t.currency}</span></label>{payments.length > 1 && <button className="remove-payment" onClick={() => setPayments((current) => current.filter((row) => row.id !== payment.id))} aria-label="حذف طريقة الدفع"><Icon name="close" size={14} /></button>}</div>)}
                <div className="remaining-row"><span>{t.remaining}</span><strong className={remaining === 0 ? 'paid' : remaining < 0 ? 'overpaid' : ''}>{money(Math.abs(remaining), lang)}{remaining < 0 ? (lang === 'ar' ? ' زيادة' : ' over') : ''}</strong></div>
              </div>

              <div className="totals-block"><div className="total-row"><span>{t.subtotal}</span><span>{money(totals.subtotal, lang)}</span></div><div className="total-row discount-total"><span>{t.discounts}</span><span>− {money(totals.discount, lang)}</span></div>{taxEnabled && <div className="total-row"><span>{pricesIncludeTax ? t.tax : (lang === 'ar' ? 'الضريبة' : 'Tax')}</span><span>{money(totals.tax, lang)}</span></div>}<div className="total-row due-row"><strong>{t.total}</strong><strong>{money(totals.total, lang)}</strong></div></div>
              <button className="checkout-button" onClick={completeSale} disabled={!cart.length}><span className="checkout-check"><Icon name="check" size={18} /></span><span>{t.complete}</span><strong>{money(totals.total, lang)}</strong></button>
              <button className="print-button" onClick={() => void printLastInvoice()} disabled={!lastInvoiceId}><Icon name="printer" size={16} />{t.print}<kbd>F4</kbd></button>
            </aside>
          </div>

          <footer className="page-footer"><span><i className="footer-dot" />{t.demoDisclaimer}</span><span>{isTauri ? 'Local database' : t.preview} · {lang === 'ar' ? 'البيانات لا تغادر هذا الجهاز' : 'Data stays on this device'}</span><span className="footer-build">NASEEJ POS <b>0.1.0</b></span></footer>
          </> : <section className="module-workspace">
            <div className="page-heading"><div><div className="eyebrow"><span className="live-dot" />{isTauri ? (lang === 'ar' ? 'قاعدة بيانات محلية' : 'Local database') : t.preview}</div><h1>{t[activeTab]}</h1><p>{activeTab === 'inventory' ? (lang === 'ar' ? 'الأرصدة على مستوى SKU مع سجل حركة وتسوية بسبب.' : 'Variant-level balances with an auditable movement ledger.') : activeTab === 'products' ? (lang === 'ar' ? 'أنشئ الصنف ثم كوّن متغيرات المقاس واللون.' : 'Create a product and configure its size/color variants.') : activeTab === 'purchases' ? (lang === 'ar' ? 'الموردون وأوامر التوريد والاستلام الجزئي.' : 'Suppliers, purchase orders, and partial receipts.') : activeTab === 'reports' ? (lang === 'ar' ? 'نطاق زمني، ملخص مبيعات، وتقارير ربح مقيّدة بالصلاحية.' : 'Date range, sales summary, and permission-gated profit reporting.') : activeTab === 'branches' ? (lang === 'ar' ? 'حدود الفروع مرتبطة بالرخصة؛ ويستطيع المالك إسناد الموظفين لكل فرع.' : 'Branch limits are enforced by the license; owners assign staff per branch.') : activeTab === 'backup' ? (lang === 'ar' ? 'نسخ .bkz مشفّر بكلمة مرور مع فحص سلامة ونسخة حماية قبل الاستعادة.' : 'Password-encrypted .bkz backups with integrity checks and a pre-restore safeguard.') : activeTab === 'returns' ? (lang === 'ar' ? 'إرجاع جزئي أو استبدال ذري، مع حفظ حركة المخزون وفرق التسوية.' : 'Partial returns or atomic exchanges with linked stock movements and settlement.') : activeTab === 'imports' ? (lang === 'ar' ? 'استيراد CSV أو XLSX مع معاينة ومراجعة قبل أي كتابة.' : 'Import CSV or XLSX with a preview and review before anything is written.') : (lang === 'ar' ? 'أنشئ حسابات موظفين بصلاحيات مقيدة دون كشف كلمات المرور.' : 'Manage staff accounts with restricted roles; passwords never leave the backend.')}</p></div><div className="date-block"><div className="date-icon"><Icon name={activeTab === 'reports' ? 'chart' : activeTab === 'branches' ? 'branch' : activeTab === 'backup' ? 'backup' : activeTab === 'returns' ? 'return' : 'box'} size={17} /></div><div><span>{lang === 'ar' ? 'نسيج POS' : 'Naseej POS'}</span><strong>{isTauri ? (lang === 'ar' ? 'محلي' : 'Offline') : t.preview}</strong></div></div></div>
            {workspaceError && <div className="module-error" role="alert">{workspaceError}</div>}
            {workspaceBusy && <div className="module-loading"><span />{lang === 'ar' ? 'جارٍ الحفظ أو التحميل…' : 'Loading or saving…'}</div>}
            {activeTab === 'inventory' && <div className="module-grid">
              <form className="module-card module-form" onSubmit={submitStockAdjustment}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'حركة مخزون موثقة' : 'Audited inventory movement'}</span><h2>{lang === 'ar' ? 'تسوية المخزون' : 'Adjust stock'}</h2></div><div className="stat-icon mint"><Icon name="box" /></div></div>
                <label>{lang === 'ar' ? 'المتغير / SKU' : 'Variant / SKU'}<select value={adjustForm.variantId} onChange={(event) => setAdjustForm((form) => ({ ...form, variantId: event.target.value }))} required><option value="">—</option>{inventoryRows.map((row) => <option key={row.variantId} value={row.variantId}>{row.sku} · {lang === 'ar' ? row.sizeCode + ' / ' + row.colorAr : row.sizeCode + ' / ' + row.colorEn}</option>)}</select></label>
                <label>{lang === 'ar' ? 'حاوية المخزون' : 'Inventory bucket'}<select value={adjustForm.bucket} onChange={(event) => setAdjustForm((form) => ({ ...form, bucket: event.target.value }))}><option value="SELLABLE">{lang === 'ar' ? 'صالح للبيع' : 'Sellable'}</option><option value="DAMAGED">{lang === 'ar' ? 'تالف' : 'Damaged'}</option><option value="QUARANTINE">{lang === 'ar' ? 'حجر / مراجعة' : 'Quarantine'}</option></select></label>
                <label>{lang === 'ar' ? 'فرق الكمية (+ / −)' : 'Quantity delta (+ / −)'}<input type="number" step="1" value={adjustForm.quantityDelta} onChange={(event) => setAdjustForm((form) => ({ ...form, quantityDelta: event.target.value }))} required /></label>
                <label>{lang === 'ar' ? 'السبب (إلزامي)' : 'Reason (required)'}<textarea value={adjustForm.reason} onChange={(event) => setAdjustForm((form) => ({ ...form, reason: event.target.value }))} required rows={3} /></label>
                <button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'تسجيل الحركة' : 'Record adjustment'}<Icon name="check" size={16} /></button>
              </form>
              <section className="module-card"><div className="module-card-head"><div><span className="modal-eyebrow">{inventoryRows.filter((row) => row.needsReorder).length} {lang === 'ar' ? 'تنبيه إعادة طلب' : 'reorder alerts'}</span><h2>{lang === 'ar' ? 'أرصدة المتغيرات' : 'Variant balances'}</h2></div><button className="filter-button" onClick={() => void openWorkspaceTab('inventory')}>{lang === 'ar' ? 'تحديث' : 'Refresh'}<Icon name="clock" size={15} /></button></div>
                <div className="module-table-wrap"><table className="module-table"><thead><tr><th>SKU</th><th>{lang === 'ar' ? 'الصنف' : 'Product'}</th><th>{lang === 'ar' ? 'مقاس / لون' : 'Size / color'}</th><th>{lang === 'ar' ? 'صالح' : 'Sellable'}</th><th>{lang === 'ar' ? 'تالف' : 'Damaged'}</th><th>{lang === 'ar' ? 'إعادة طلب' : 'Reorder'}</th></tr></thead><tbody>{inventoryRows.map((row) => <tr key={row.variantId}><td className="mono">{row.sku}</td><td>{lang === 'ar' ? row.productNameAr : row.productNameEn}</td><td>{row.sizeCode} · {lang === 'ar' ? row.colorAr : row.colorEn}</td><td>{row.sellable}</td><td>{row.damaged}</td><td><span className={`stock-pill ${row.needsReorder ? 'low' : ''}`}>{row.sellable} / {row.reorderLevel}</span></td></tr>)}{inventoryRows.length === 0 && <tr><td colSpan={6} className="empty-table">{lang === 'ar' ? 'لا توجد متغيرات مخزون بعد.' : 'No variant stock yet.'}</td></tr>}</tbody></table></div>
              </section>
            </div>}
            {activeTab === 'products' && <div className="module-grid">
              <form className="module-card module-form product-module-form" onSubmit={saveProduct}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'كتالوج الملابس' : 'Apparel catalog'}</span><h2>{lang === 'ar' ? 'صنف جديد ومتغيراته' : 'New product and variants'}</h2></div><div className="stat-icon lilac"><Icon name="grid" /></div></div>
                {!catalogSetup ? <div className="auth-warning">{lang === 'ar' ? 'حمّل خيارات الكتالوج من قاعدة البيانات المحلية.' : 'Load catalog options from the local database.'}</div> : <>
                  <div className="module-fields-two"><label>{lang === 'ar' ? 'الفئة' : 'Category'}<select value={productForm.categoryId} onChange={(event) => setProductForm((form) => ({ ...form, categoryId: event.target.value }))} required>{catalogSetup.categories.map((option) => <option key={option.id} value={option.id}>{lang === 'ar' ? option.nameAr : option.nameEn}</option>)}</select></label><label>{lang === 'ar' ? 'الماركة' : 'Brand'}<select value={productForm.brandId} onChange={(event) => setProductForm((form) => ({ ...form, brandId: event.target.value }))}><option value="">—</option>{catalogSetup.brands.map((option) => <option key={option.id} value={option.id}>{lang === 'ar' ? option.nameAr : option.nameEn}</option>)}</select></label></div>
                  <div className="module-fields-two"><label>{lang === 'ar' ? 'الاسم بالعربية' : 'Arabic name'}<input value={productForm.nameAr} onChange={(event) => setProductForm((form) => ({ ...form, nameAr: event.target.value }))} required /></label><label>{lang === 'ar' ? 'الاسم بالإنجليزية' : 'English name'}<input value={productForm.nameEn} onChange={(event) => setProductForm((form) => ({ ...form, nameEn: event.target.value }))} required /></label></div>
                  <div className="module-fields-two"><label>{lang === 'ar' ? 'تكلفة الوحدة (ج.م)' : 'Unit cost (EGP)'}<input type="number" min="0" step="0.01" value={productForm.baseCost} onChange={(event) => setProductForm((form) => ({ ...form, baseCost: event.target.value }))} required /></label><label>{lang === 'ar' ? 'سعر البيع (ج.م)' : 'Sale price (EGP)'}<input type="number" min="0" step="0.01" value={productForm.basePrice} onChange={(event) => setProductForm((form) => ({ ...form, basePrice: event.target.value }))} required /></label></div>
                  <div className="module-fields-two"><label>{lang === 'ar' ? 'الموسم (اختياري)' : 'Season (optional)'}<select value={productForm.seasonId} onChange={(event) => setProductForm((form) => ({ ...form, seasonId: event.target.value }))}><option value="">—</option>{catalogSetup.seasons.map((option) => <option key={option.id} value={option.id}>{lang === 'ar' ? option.nameAr : option.nameEn}</option>)}</select></label><label>{lang === 'ar' ? 'حد إعادة الطلب لكل متغير' : 'Reorder level per variant'}<input type="number" min="0" step="1" value={productForm.reorderLevel} onChange={(event) => setProductForm((form) => ({ ...form, reorderLevel: event.target.value }))} /></label></div>
                  <div className="variant-pick-grid"><div><strong>{lang === 'ar' ? 'المقاسات' : 'Sizes'}</strong><div className="option-chip-grid">{catalogSetup.sizes.map((option) => <label className="option-chip" key={option.id}><input type="checkbox" checked={productForm.sizeIds.includes(option.id)} onChange={(event) => setProductForm((form) => ({ ...form, sizeIds: event.target.checked ? [...form.sizeIds, option.id] : form.sizeIds.filter((id) => id !== option.id) }))} /><span>{option.code}</span></label>)}</div></div><div><strong>{lang === 'ar' ? 'الألوان' : 'Colors'}</strong><div className="option-chip-grid">{catalogSetup.colors.map((option) => <label className="option-chip" key={option.id}><input type="checkbox" checked={productForm.colorIds.includes(option.id)} onChange={(event) => setProductForm((form) => ({ ...form, colorIds: event.target.checked ? [...form.colorIds, option.id] : form.colorIds.filter((id) => id !== option.id) }))} /><span><i className="color-dot" style={{ backgroundColor: option.colorHex ?? '#aaa' }} />{lang === 'ar' ? option.nameAr : option.nameEn}</span></label>)}</div></div></div>
                  <div className="module-fields-two"><label>{lang === 'ar' ? 'رصيد افتتاحي لكل متغير' : 'Opening units per variant'}<input type="number" min="0" step="1" value={productForm.openingQuantity} onChange={(event) => setProductForm((form) => ({ ...form, openingQuantity: event.target.value }))} /></label><label>{lang === 'ar' ? 'سبب الرصيد الافتتاحي' : 'Opening-stock reason'}<input value={productForm.openingReason} onChange={(event) => setProductForm((form) => ({ ...form, openingReason: event.target.value }))} /></label></div>
                  <div className="auth-helper">{productForm.sizeIds.length * productForm.colorIds.length} {lang === 'ar' ? 'متغيرًا سيتم إنشاؤه' : 'variants will be created'} · {lang === 'ar' ? 'يُولّد SKU لكل مقاس ولون' : 'SKU is generated for each size/color'}</div>
                  <button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'حفظ الصنف' : 'Save product'}<Icon name="check" size={16} /></button>
                </>}
              </form>
              <section className="module-card"><div className="module-card-head"><div><span className="modal-eyebrow">{catalogItems.length} {lang === 'ar' ? 'أصناف' : 'products'}</span><h2>{lang === 'ar' ? 'الكتالوج الحالي' : 'Current catalog'}</h2></div></div><div className="module-product-list">{catalogItems.map((product) => <div className="module-product-row" key={product.id}><div className={`line-thumb ${product.art}`}><div className="mini-garment" /></div><div><strong>{lang === 'ar' ? product.nameAr : product.nameEn}</strong><small>{product.variants.length} {lang === 'ar' ? 'متغير' : 'variants'} · {product.brand}</small></div><b>{money(product.priceMinor, lang)}</b></div>)}</div></section>
            </div>}
            {activeTab === 'purchases' && <div className="module-grid">
              <form className="module-card module-form" onSubmit={createSupplier}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'بيانات التواصل مشفّرة' : 'Encrypted contact fields'}</span><h2>{lang === 'ar' ? 'مورد جديد' : 'New supplier'}</h2></div><div className="stat-icon peach"><Icon name="user" /></div></div><label>{lang === 'ar' ? 'رمز المورد' : 'Supplier code'}<input value={supplierForm.supplierCode} onChange={(event) => setSupplierForm((form) => ({ ...form, supplierCode: event.target.value }))} required /></label><label>{lang === 'ar' ? 'الاسم' : 'Name'}<input value={supplierForm.name} onChange={(event) => setSupplierForm((form) => ({ ...form, name: event.target.value }))} required /></label><div className="module-fields-two"><label>{lang === 'ar' ? 'الهاتف' : 'Phone'}<input type="tel" value={supplierForm.phone} onChange={(event) => setSupplierForm((form) => ({ ...form, phone: event.target.value }))} /></label><label>{lang === 'ar' ? 'البريد' : 'Email'}<input type="email" value={supplierForm.email} onChange={(event) => setSupplierForm((form) => ({ ...form, email: event.target.value }))} /></label></div><button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'حفظ المورد' : 'Save supplier'}<Icon name="check" size={16} /></button></form>
              <form className="module-card module-form" onSubmit={createPurchaseOrder}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'التكلفة لا تظهر للكاشير' : 'Cost is hidden from cashiers'}</span><h2>{lang === 'ar' ? 'أمر توريد' : 'Purchase order'}</h2></div><div className="stat-icon mint"><Icon name="box" /></div></div><label>{lang === 'ar' ? 'المورد' : 'Supplier'}<select value={purchaseForm.supplierId} onChange={(event) => setPurchaseForm((form) => ({ ...form, supplierId: event.target.value }))} required><option value="">—</option>{suppliers.map((supplier) => <option key={supplier.id} value={supplier.id}>{supplier.name} · {supplier.supplierCode}</option>)}</select></label><label>{lang === 'ar' ? 'رقم أمر التوريد' : 'Order number'}<input value={purchaseForm.orderNo} onChange={(event) => setPurchaseForm((form) => ({ ...form, orderNo: event.target.value }))} required /></label><label>{lang === 'ar' ? 'المتغير' : 'Variant'}<select value={purchaseForm.variantId} onChange={(event) => setPurchaseForm((form) => ({ ...form, variantId: event.target.value }))} required><option value="">—</option>{catalogItems.flatMap((product) => product.variants.map((variant) => <option key={variant.id} value={variant.id}>{variant.sku} · {lang === 'ar' ? product.nameAr : product.nameEn} · {variant.size}/{variant.colorEn}</option>))}</select></label><div className="module-fields-two"><label>{lang === 'ar' ? 'الكمية' : 'Quantity'}<input type="number" min="1" step="1" value={purchaseForm.quantity} onChange={(event) => setPurchaseForm((form) => ({ ...form, quantity: event.target.value }))} required /></label><label>{lang === 'ar' ? 'تكلفة الوحدة (ج.م)' : 'Unit cost (EGP)'}<input type="number" min="0" step="0.01" value={purchaseForm.unitCost} onChange={(event) => setPurchaseForm((form) => ({ ...form, unitCost: event.target.value }))} required /></label></div><label>{lang === 'ar' ? 'ضريبة تكلفة التوريد (%)' : 'Purchase tax rate (%)'}<input type="number" min="0" max="100" step="0.01" value={purchaseForm.taxRate} onChange={(event) => setPurchaseForm((form) => ({ ...form, taxRate: event.target.value }))} required /></label><button className="auth-submit" type="submit" disabled={workspaceBusy || !suppliers.length || !catalogItems.length}>{lang === 'ar' ? 'إنشاء الأمر' : 'Create order'}<Icon name="check" size={16} /></button></form>
              <form className="module-card module-form" onSubmit={receiveGoods}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'يزيد المخزون عند الاستلام فقط' : 'Stock posts only on receipt'}</span><h2>{lang === 'ar' ? 'استلام بضاعة' : 'Receive goods'}</h2></div><div className="stat-icon lilac"><Icon name="box" /></div></div><label>{lang === 'ar' ? 'أمر التوريد المفتوح' : 'Open purchase order'}<select value={receiptForm.purchaseOrderId} onChange={(event) => { const order = purchaseOrders.find((item) => item.id === event.target.value); setReceiptForm((form) => ({ ...form, purchaseOrderId: event.target.value, lineId: order?.lines[0]?.id ?? '' })); }} required><option value="">—</option>{purchaseOrders.map((order) => <option key={order.id} value={order.id}>{order.orderNo} · {order.supplierName}</option>)}</select></label><label>{lang === 'ar' ? 'بند الأمر' : 'Order line'}<select value={receiptForm.lineId} onChange={(event) => setReceiptForm((form) => ({ ...form, lineId: event.target.value }))} required><option value="">—</option>{purchaseOrders.find((item) => item.id === receiptForm.purchaseOrderId)?.lines.map((line) => <option key={line.id} value={line.id}>{line.sku} · {line.productNameEn} · {line.quantityOpen} {lang === 'ar' ? 'متبقي' : 'open'}</option>)}</select></label><div className="module-fields-two"><label>{lang === 'ar' ? 'الكمية المستلمة' : 'Received quantity'}<input type="number" min="1" max={purchaseOrders.find((item) => item.id === receiptForm.purchaseOrderId)?.lines.find((line) => line.id === receiptForm.lineId)?.quantityOpen ?? 1} step="1" value={receiptForm.quantity} onChange={(event) => setReceiptForm((form) => ({ ...form, quantity: event.target.value }))} required /></label><label>{lang === 'ar' ? 'مرجع فاتورة المورد' : 'Supplier invoice ref.'}<input value={receiptForm.supplierInvoiceRef} onChange={(event) => setReceiptForm((form) => ({ ...form, supplierInvoiceRef: event.target.value }))} /></label></div><button className="auth-submit" type="submit" disabled={workspaceBusy || !purchaseOrders.length}>{lang === 'ar' ? 'تسجيل الاستلام' : 'Post receipt'}<Icon name="check" size={16} /></button></form>
              <section className="module-card"><div className="module-card-head"><div><span className="modal-eyebrow">{purchaseOrders.length} {lang === 'ar' ? 'أوامر مفتوحة' : 'open orders'}</span><h2>{lang === 'ar' ? 'المتبقي للاستلام' : 'Outstanding lines'}</h2></div></div><div className="module-product-list">{purchaseOrders.flatMap((order) => order.lines.map((line) => <div className="module-product-row" key={line.id}><div className="stat-icon peach"><Icon name="box" size={16} /></div><div><strong>{line.sku} · {line.productNameEn}</strong><small>{order.orderNo} · {order.supplierName} · {line.sizeCode} · {line.quantityOpen} {lang === 'ar' ? 'متبقي' : 'open'}</small></div><b>{money(line.unitCostMinor, lang)}</b></div>))}{purchaseOrders.length === 0 && <div className="empty-table">{lang === 'ar' ? 'لا توجد أوامر مفتوحة.' : 'No open purchase orders.'}</div>}</div></section>
            </div>}
            {activeTab === 'users' && <div className="module-grid">
              <form className="module-card module-form" onSubmit={createUser}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'صلاحية المالك' : 'Owner-only permission'}</span><h2>{lang === 'ar' ? 'إنشاء حساب موظف' : 'Create staff account'}</h2></div><div className="stat-icon lilac"><Icon name="user" /></div></div><label>{lang === 'ar' ? 'اسم المستخدم' : 'Username'}<input value={userForm.username} onChange={(event) => setUserForm((form) => ({ ...form, username: event.target.value }))} required minLength={3} maxLength={40} /></label><label>{lang === 'ar' ? 'الاسم الظاهر' : 'Display name'}<input value={userForm.displayName} onChange={(event) => setUserForm((form) => ({ ...form, displayName: event.target.value }))} required /></label><label>{lang === 'ar' ? 'كلمة المرور المؤقتة (12 حرفًا على الأقل)' : 'Temporary password (at least 12 characters)'}<input type="password" value={userForm.password} onChange={(event) => setUserForm((form) => ({ ...form, password: event.target.value }))} required minLength={12} autoComplete="new-password" /></label><label>{lang === 'ar' ? 'الدور' : 'Role'}<select value={userForm.roleCode} onChange={(event) => setUserForm((form) => ({ ...form, roleCode: event.target.value }))}><option value="cashier">{lang === 'ar' ? 'كاشير' : 'Cashier'}</option><option value="manager">{lang === 'ar' ? 'مدير' : 'Manager'}</option></select></label><div className="auth-helper">{lang === 'ar' ? `عدد المستخدمين النشطين: ${users.filter((user) => user.isActive).length}${licenseStatus?.limits.max_users ? ` / ${licenseStatus.limits.max_users}` : ''}. كلمة المرور تُهشّ بـ Argon2id داخل Rust.` : `Active users: ${users.filter((user) => user.isActive).length}${licenseStatus?.limits.max_users ? ` / ${licenseStatus.limits.max_users}` : ''}. Passwords are Argon2id-hashed in Rust.`}</div><button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'إنشاء المستخدم' : 'Create user'}<Icon name="check" size={16} /></button></form>
              <section className="module-card"><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'الكاشير لا يدير المستخدمين' : 'Cashiers cannot administer users'}</span><h2>{lang === 'ar' ? 'حسابات الفريق' : 'Team accounts'}</h2></div></div><div className="module-product-list">{users.map((user) => <div className="module-product-row user-row" key={user.id}><div className="avatar">{user.displayName.slice(0, 1)}</div><div><strong>{user.displayName} · {user.username}</strong><small>{user.roleCode} · {user.isActive ? (lang === 'ar' ? 'نشط' : 'Active') : (lang === 'ar' ? 'موقوف' : 'Inactive')}</small></div><select aria-label={lang === 'ar' ? 'الدور' : 'Role'} value={user.roleCode} disabled={user.roleCode === 'owner' || workspaceBusy} onChange={(event) => void changeUserRole(user.id, event.target.value)}><option value="owner">owner</option><option value="cashier">cashier</option><option value="manager">manager</option></select><button className="filter-button" disabled={user.roleCode === 'owner' || workspaceBusy} onClick={() => void toggleUserActive(user)}>{user.isActive ? (lang === 'ar' ? 'إيقاف' : 'Disable') : (lang === 'ar' ? 'تفعيل' : 'Enable')}</button></div>)}{users.length === 0 && <div className="empty-table">{lang === 'ar' ? 'لا توجد حسابات محلية.' : 'No local accounts.'}</div>}</div><div className="auth-helper">{lang === 'ar' ? 'لا يمكن إيقاف المالك الأخير. المدير لا يملك صلاحية إدارة المستخدمين.' : 'The last active owner cannot be disabled. Managers cannot manage user accounts.'}</div></section>
            </div>}
            {activeTab === 'users' && <div className="module-grid">
              <section className="module-card module-form">
                <div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'برنامج اختياري' : 'Optional programme'}</span><h2>{lang === 'ar' ? 'نقاط الولاء' : 'Loyalty points'}</h2></div><div className="stat-icon mint"><Icon name="tag" /></div></div>
                <div className="tax-choice"><button type="button" className={loyaltyForm.enabled ? 'chosen' : ''} onClick={() => setLoyaltyForm((form) => ({ ...form, enabled: true }))}>{lang === 'ar' ? 'مفعّل' : 'Enabled'}</button><button type="button" className={!loyaltyForm.enabled ? 'chosen' : ''} onClick={() => setLoyaltyForm((form) => ({ ...form, enabled: false }))}>{lang === 'ar' ? 'موقوف' : 'Off'}</button></div>
                <div className="module-fields-two">
                  <label>{lang === 'ar' ? 'نقاط لكل ١ ج.م' : 'Points per 1 EGP'}<input type="number" min="0" max="1000" step="1" value={loyaltyForm.pointsPerCurrencyUnit} onChange={(event) => setLoyaltyForm((form) => ({ ...form, pointsPerCurrencyUnit: event.target.value }))} /></label>
                  <label>{lang === 'ar' ? 'قيمة النقطة (ج.م)' : 'Point value (EGP)'}<input type="number" min="0" max="1000" step="0.01" value={loyaltyForm.redeemValue} onChange={(event) => setLoyaltyForm((form) => ({ ...form, redeemValue: event.target.value }))} /></label>
                </div>
                <label>{lang === 'ar' ? 'أقل عدد نقاط للاستبدال' : 'Minimum points to redeem'}<input type="number" min="0" step="1" value={loyaltyForm.minRedeemPoints} onChange={(event) => setLoyaltyForm((form) => ({ ...form, minRedeemPoints: event.target.value }))} /></label>
                <button className="auth-submit" type="button" disabled={workspaceBusy} onClick={() => void saveLoyaltySettings()}>{lang === 'ar' ? 'حفظ إعدادات الولاء' : 'Save loyalty settings'}<Icon name="check" size={16} /></button>
                {!loyaltySettings.isEnabled && <small className="auth-helper">{lang === 'ar' ? 'البرنامج موقوف افتراضيًا ولا تُسجَّل نقاط.' : 'The programme is off by default and no points are recorded.'}</small>}
              </section>
              <section className="module-card">
                <div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'نقاط ورصيد' : 'Points and balance'}</span><h2>{lang === 'ar' ? 'رصيد العميل' : 'Customer balance'}</h2></div><div className="stat-icon lilac"><Icon name="user" /></div></div>
                <label>{lang === 'ar' ? 'رقم هاتف العميل' : 'Customer phone'}<input value={loyaltyPhone} onChange={(event) => setLoyaltyPhone(event.target.value)} placeholder="01000000000" /></label>
                <button className="filter-button" type="button" disabled={workspaceBusy || !loyaltyPhone.trim()} onClick={() => void lookupLoyalty()}>{lang === 'ar' ? 'بحث عن العميل' : 'Look up customer'}<Icon name="search" size={15} /></button>
                {customerLoyalty && <div className="report-cards"><div><span>{lang === 'ar' ? 'النقاط' : 'Points'}</span><strong>{customerLoyalty.points}</strong></div><div><span>{lang === 'ar' ? 'قيمتها' : 'Value'}</span><strong>{money(customerLoyalty.valueMinor, lang)}</strong></div></div>}
                {storeCredits.length > 0 && <div className="module-product-list">{storeCredits.map((credit) => <div className="module-product-row" key={credit.id}><div className="stat-icon mint"><Icon name="wallet" size={16} /></div><div><strong>{credit.remainingMinor > 0 ? money(credit.remainingMinor, lang) : (lang === 'ar' ? 'مستخدم بالكامل' : 'Fully used')}</strong><small>{lang === 'ar' ? 'أُصدر' : 'Issued'} {new Date(credit.createdAt).toLocaleDateString(lang === 'ar' ? 'ar-EG' : 'en-GB')} · {money(credit.issuedMinor, lang)}</small></div></div>)}</div>}
              </section>
            </div>}

            {activeTab === 'branches' && <div className="module-grid">
              <form className="module-card module-form" onSubmit={createBranch}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'حد الرخصة' : 'License limit'}</span><h2>{lang === 'ar' ? 'إنشاء فرع' : 'Create a branch'}</h2></div><div className="stat-icon lilac"><Icon name="branch" /></div></div><div className="auth-helper">{lang === 'ar' ? `الفروع المستخدمة: ${branches.length}${licenseStatus?.limits.max_branches ? ` / ${licenseStatus.limits.max_branches}` : ''}. سيُنسخ إعداد المقاسات والألوان والفئات من الفرع النشط.` : `Branches used: ${branches.length}${licenseStatus?.limits.max_branches ? ` / ${licenseStatus.limits.max_branches}` : ''}. Sizes, colors, and categories are copied from the active branch.`}</div><label>{lang === 'ar' ? 'رمز الفرع' : 'Branch code'}<input value={branchForm.code} onChange={(event) => setBranchForm((form) => ({ ...form, code: event.target.value.toUpperCase() }))} minLength={2} maxLength={16} required /></label><div className="module-fields-two"><label>{lang === 'ar' ? 'اسم الفرع بالعربية' : 'Arabic branch name'}<input value={branchForm.nameAr} onChange={(event) => setBranchForm((form) => ({ ...form, nameAr: event.target.value }))} required /></label><label>{lang === 'ar' ? 'اسم الفرع بالإنجليزية' : 'English branch name'}<input value={branchForm.nameEn} onChange={(event) => setBranchForm((form) => ({ ...form, nameEn: event.target.value }))} required /></label></div><button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'إنشاء الفرع' : 'Create branch'}<Icon name="check" size={16} /></button></form>
              <section className="module-card"><div className="module-card-head"><div><span className="modal-eyebrow">{branches.length} {lang === 'ar' ? 'فروع متاحة لحسابك' : 'branches assigned to you'}</span><h2>{lang === 'ar' ? 'الفروع' : 'Branches'}</h2></div></div><div className="module-product-list">{branches.map((branch) => <div className="module-product-row" key={branch.id}><div className="stat-icon mint"><Icon name="branch" size={16} /></div><div><strong>{branch.code} · {lang === 'ar' ? branch.nameAr : branch.nameEn}</strong><small>{branch.id === activeBranchId ? (lang === 'ar' ? 'الفرع النشط' : 'Active branch') : (lang === 'ar' ? 'متاح للتبديل' : 'Available')}</small></div><button className="filter-button" disabled={branch.id === activeBranchId || workspaceBusy} onClick={() => void changeBranch(branch.id)}>{lang === 'ar' ? 'فتح' : 'Switch'}</button></div>)}</div></section>
              <form className="module-card module-form" onSubmit={assignUserToBranch}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'مدير الفرع فقط' : 'Branch-owner only'}</span><h2>{lang === 'ar' ? 'إسناد موظف لفرع' : 'Assign staff to a branch'}</h2></div><div className="stat-icon peach"><Icon name="user" /></div></div><label>{lang === 'ar' ? 'الفرع' : 'Branch'}<select value={branchAssignment.branchId} onChange={(event) => setBranchAssignment((form) => ({ ...form, branchId: event.target.value }))} required><option value="">—</option>{branches.map((branch) => <option key={branch.id} value={branch.id}>{branch.code} · {lang === 'ar' ? branch.nameAr : branch.nameEn}</option>)}</select></label><label>{lang === 'ar' ? 'المستخدم' : 'User'}<select value={branchAssignment.userId} onChange={(event) => setBranchAssignment((form) => ({ ...form, userId: event.target.value }))} required><option value="">—</option>{users.filter((user) => user.isActive && user.roleCode !== 'owner').map((user) => <option key={user.id} value={user.id}>{user.displayName} · {user.username}{user.roleCode ? ` · ${user.roleCode}` : ''}</option>)}</select></label><label>{lang === 'ar' ? 'الدور في الفرع' : 'Role in branch'}<select value={branchAssignment.roleCode} onChange={(event) => setBranchAssignment((form) => ({ ...form, roleCode: event.target.value }))}><option value="cashier">{lang === 'ar' ? 'كاشير' : 'Cashier'}</option><option value="manager">{lang === 'ar' ? 'مدير' : 'Manager'}</option></select></label><button className="auth-submit" type="submit" disabled={workspaceBusy || !users.some((user) => user.isActive && user.roleCode !== 'owner')}>{lang === 'ar' ? 'حفظ الإسناد' : 'Save assignment'}<Icon name="check" size={16} /></button></form>
            </div>}
            {activeTab === 'imports' && <div className="module-grid">
              <section className="module-card module-form">
                <div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'لا شيء يُكتب قبل الاعتماد' : 'Nothing is written before approval'}</span><h2>{lang === 'ar' ? 'ملف البضاعة الافتتاحية' : 'Opening inventory file'}</h2></div><div className="stat-icon lilac"><Icon name="boxes" /></div></div>
                <p className="auth-helper">{lang === 'ar' ? 'الأعمدة المقبولة: اسم الصنف، الفئة، السعر، التكلفة، المقاس، اللون، الكود، الباركود، الرصيد. الملف يبقى على جهازك ولا يُرسل لأي خدمة.' : 'Accepted columns: item name, category, price, cost, size, color, SKU, barcode, quantity. The file stays on this machine and is never uploaded.'}</p>
                <input ref={importFileRef} type="file" accept=".csv,.xlsx" disabled={workspaceBusy} onChange={(event) => void chooseImportFile(event.target.files?.[0] ?? null)} />
                {importPreview && (
                  <div className="import-preview">
                    <div className="report-cards">
                      <div><span>{lang === 'ar' ? 'إجمالي الأسطر' : 'Total rows'}</span><strong>{importPreview.totalRows}</strong></div>
                      <div><span>{lang === 'ar' ? 'مقبول' : 'Accepted'}</span><strong>{importPreview.acceptedRows}</strong></div>
                      <div><span>{lang === 'ar' ? 'مرفوض' : 'Rejected'}</span><strong>{importPreview.rejectedRows}</strong></div>
                    </div>
                    {importPreview.errors.length > 0 && <div className="auth-warning">{importPreview.errors.slice(0, 8).map((error) => <div key={`${error.rowNumber}-${error.code}`}>{describeImportError(error.code, error.rowNumber)}</div>)}{importPreview.errors.length > 8 && <div>{lang === 'ar' ? `و${importPreview.errors.length - 8} خطأ آخر` : `and ${importPreview.errors.length - 8} more`}</div>}</div>}
                    <div className="module-product-list">{importPreview.accepted.slice(0, 6).map((row) => <div className="module-product-row" key={`${row.rowNumber}-${row.sku}`}><div><strong>{row.nameAr} · {row.sizeCode} / {row.color}</strong><small>{row.sku} · {money(row.priceMinor, lang)} · {row.openingQty} {t.units}</small></div></div>)}{importPreview.accepted.length > 6 && <div className="empty-table">{lang === 'ar' ? `و${importPreview.accepted.length - 6} سطرًا آخر` : `and ${importPreview.accepted.length - 6} more rows`}</div>}</div>
                    <button className="auth-submit" type="button" disabled={workspaceBusy || importPreview.acceptedRows === 0} onClick={() => void approveImport()}>{lang === 'ar' ? 'اعتماد الاستيراد وكتابة البيانات' : 'Approve and write the import'}<Icon name="check" size={16} /></button>
                  </div>
                )}
                {importResult && <div className="auth-helper">{lang === 'ar' ? `تم إنشاء ${importResult.productsCreated} صنفًا و${importResult.variantsCreated} متغيرًا و${importResult.stockMovements} حركة مخزون.` : `Created ${importResult.productsCreated} products, ${importResult.variantsCreated} variants, and ${importResult.stockMovements} stock movements.`}</div>}
              </section>
              <section className="module-card">
                <div className="module-card-head"><div><span className="modal-eyebrow">{importBatches.length} {lang === 'ar' ? 'دفعة' : 'batches'}</span><h2>{lang === 'ar' ? 'سجل الاستيراد' : 'Import history'}</h2></div><button className="filter-button" onClick={() => void openWorkspaceTab('imports')}>{lang === 'ar' ? 'تحديث' : 'Refresh'}<Icon name="clock" size={15} /></button></div>
                <div className="module-product-list">{importBatches.map((batch) => <div className="module-product-row" key={batch.id}><div className={`stat-icon ${batch.status === 'COMMITTED' ? 'mint' : 'peach'}`}><Icon name="box" size={16} /></div><div><strong>{batch.sourceKind} · {batch.status}</strong><small>{batch.acceptedRows} {lang === 'ar' ? 'مقبول' : 'accepted'} · {batch.rejectedRows} {lang === 'ar' ? 'مرفوض' : 'rejected'} · {new Date(batch.createdAt).toLocaleString(lang === 'ar' ? 'ar-EG' : 'en-GB')}</small></div></div>)}{importBatches.length === 0 && <div className="empty-table">{lang === 'ar' ? 'لا يوجد استيراد سابق.' : 'No imports yet.'}</div>}</div>
              </section>
            </div>}

            {activeTab === 'backup' && <div className="module-grid backup-grid">
              <form className="module-card module-form" onSubmit={createBackup}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'تشفير بكلمة مرور' : 'Passphrase encryption'}</span><h2>{lang === 'ar' ? 'إنشاء نسخة احتياطية' : 'Create encrypted backup'}</h2></div><div className="stat-icon mint"><Icon name="backup" /></div></div><div className="auth-warning">{lang === 'ar' ? 'أدخل مسارًا مطلقًا لملف .bkz. كلمة المرور لا تُحفظ؛ احتفظ بها منفصلة عن ملف النسخة.' : 'Enter an absolute .bkz path. The password is never stored; keep it separate from the backup file.'}</div><label>{lang === 'ar' ? 'مسار الحفظ' : 'Destination path'}<input className="mono" value={backupForm.destinationPath} onChange={(event) => setBackupForm((form) => ({ ...form, destinationPath: event.target.value }))} placeholder="E:\\NaseejPOS\\backup.bkz" required /></label><label>{lang === 'ar' ? 'نوع الوجهة' : 'Destination type'}<select value={backupForm.destinationKind} onChange={(event) => setBackupForm((form) => ({ ...form, destinationKind: event.target.value }))}><option value="LOCAL">{lang === 'ar' ? 'قرص محلي' : 'Local disk'}</option><option value="USB">USB</option><option value="EXTERNAL">{lang === 'ar' ? 'قرص خارجي' : 'External drive'}</option></select></label><label>{lang === 'ar' ? 'كلمة مرور النسخة (12 حرفًا على الأقل)' : 'Backup password (at least 12 characters)'}<input type="password" minLength={12} value={backupForm.password} onChange={(event) => setBackupForm((form) => ({ ...form, password: event.target.value }))} autoComplete="new-password" required /></label><button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'تشفير وحفظ النسخة' : 'Encrypt and save backup'}<Icon name="backup" size={16} /></button></form>
              <form className="module-card module-form" onSubmit={restoreBackup}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'تحقق قبل الاستبدال' : 'Validated before replacement'}</span><h2>{lang === 'ar' ? 'استعادة نسخة' : 'Restore backup'}</h2></div><div className="stat-icon peach"><Icon name="backup" /></div></div><div className="auth-warning">{lang === 'ar' ? 'تُرفض النسخ الخاطئة أو المتلاعب بها، ويُنشأ ملف حماية مشفّر قبل استبدال قاعدة البيانات.' : 'Corrupt or tampered files are rejected. A protected encrypted backup is created before replacing the live database.'}</div><label>{lang === 'ar' ? 'مسار ملف .bkz' : '.bkz file path'}<input className="mono" value={restoreForm.sourcePath} onChange={(event) => setRestoreForm((form) => ({ ...form, sourcePath: event.target.value }))} placeholder="E:\\NaseejPOS\\backup.bkz" required /></label><label>{lang === 'ar' ? 'كلمة مرور النسخة' : 'Backup password'}<input type="password" value={restoreForm.password} onChange={(event) => setRestoreForm((form) => ({ ...form, password: event.target.value }))} autoComplete="current-password" required /></label><button className="auth-submit danger-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'تحقق واستعد' : 'Verify and restore'}<Icon name="backup" size={16} /></button></form>
              <section className="module-card"><div className="module-card-head"><div><span className="modal-eyebrow">{backupHistory.length} {lang === 'ar' ? 'عمليات حديثة' : 'recent operations'}</span><h2>{lang === 'ar' ? 'سجل النسخ والاستعادة' : 'Backup history'}</h2></div><button className="filter-button" onClick={() => void openWorkspaceTab('backup')}>{lang === 'ar' ? 'تحديث' : 'Refresh'}<Icon name="clock" size={15} /></button></div><div className="module-product-list">{backupHistory.map((row) => <div className="module-product-row" key={row.id}><div className={`stat-icon ${row.status === 'FAILED' ? 'peach' : 'mint'}`}><Icon name={row.status === 'FAILED' ? 'close' : 'backup'} size={16} /></div><div><strong>{row.filename ?? row.backupKind} · {row.status}</strong><small>{row.backupKind} · {new Date(row.startedAt).toLocaleString(lang === 'ar' ? 'ar-EG' : 'en-GB')}{row.byteSize ? ` · ${(row.byteSize / 1024 / 1024).toFixed(1)} MB` : ''}</small></div></div>)}{backupHistory.length === 0 && <div className="empty-table">{lang === 'ar' ? 'لا توجد عمليات مسجلة بعد.' : 'No backup operations recorded yet.'}</div>}</div></section>
            </div>}
            {activeTab === 'returns' && <>
              {returnPolicy && <div className="policy-strip"><span className="policy-strip-label">{lang === 'ar' ? 'سياسة المرتجعات' : 'Return policy'}</span><span>{lang === 'ar' ? `مدة الاسترجاع ${returnPolicy.periodDays} يومًا` : `${returnPolicy.periodDays} day return window`}</span><span>{returnPolicy.allowDiscountedItems ? (lang === 'ar' ? 'الأصناف المخفّضة قابلة للإرجاع' : 'discounted items may be returned') : (lang === 'ar' ? 'الأصناف المخفّضة غير قابلة للإرجاع' : 'discounted items are refused')}</span><span>{lang === 'ar' ? `الإرجاع للمخزون: ${returnPolicy.restockConditions.map((condition) => condition === 'UNOPENED' ? 'غير مفتوحة' : 'بالبطاقة').join('، ')}` : `Restock: ${returnPolicy.restockConditions.join(', ')}`}</span></div>}
              <div className="module-grid returns-grid">
                <form className="module-card module-form" onSubmit={findReturnInvoice}><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'إرجاع جزئي' : 'Partial returns'}</span><h2>{lang === 'ar' ? 'العثور على الفاتورة الأصلية' : 'Find original invoice'}</h2></div><div className="stat-icon peach"><Icon name="return" /></div></div><label>{lang === 'ar' ? 'رقم الفاتورة' : 'Invoice number'}<input value={returnInvoiceNo} onChange={(event) => setReturnInvoiceNo(event.target.value)} placeholder="NS-YYYYMMDD-0001" required /></label><button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'تحميل البنود القابلة للإرجاع' : 'Load returnable lines'}<Icon name="search" size={16} /></button>{returnableInvoice && <div className="auth-helper">{lang === 'ar' ? `الفاتورة ${returnableInvoice.invoiceNo} · ${returnableInvoice.status} · ${new Date(returnableInvoice.createdAt).toLocaleDateString('ar-EG')}` : `Invoice ${returnableInvoice.invoiceNo} · ${returnableInvoice.status} · ${new Date(returnableInvoice.createdAt).toLocaleDateString('en-GB')}`}</div>}</form>
                <section className="module-card return-lines-card"><div className="module-card-head"><div><span className="modal-eyebrow">{selectedReturnLines.length} {lang === 'ar' ? 'بنود مختارة' : 'selected lines'}</span><h2>{lang === 'ar' ? 'البنود وحالة القطعة' : 'Items and condition'}</h2></div></div>{!returnableInvoice ? <div className="empty-table">{lang === 'ar' ? 'ابحث عن فاتورة لعرض البنود.' : 'Search for an invoice to load its lines.'}</div> : <div className="return-line-list">{returnableInvoice.lines.map((line) => {
                  const selection = returnSelections[line.invoiceLineId] ?? { selected: false, quantity: '1', itemCondition: 'NEW_TAGS', disposition: 'RESTOCK' };
                  return <div className={`return-line-card ${selection.selected ? 'selected' : ''}`} key={line.invoiceLineId}>
                    <label className="return-line-head"><input type="checkbox" checked={selection.selected} disabled={!line.remainingQuantity || workspaceBusy} onChange={(event) => setReturnSelections((current) => ({ ...current, [line.invoiceLineId]: { ...selection, selected: event.target.checked, quantity: selection.quantity || '1' } }))} /><span><strong>{line.productNameAr} · {line.size} · {line.color}</strong><small>{line.sku} · {lang === 'ar' ? 'المباع' : 'sold'} {line.quantity} · {lang === 'ar' ? 'المُعاد' : 'returned'} {line.returnedQuantity} · {lang === 'ar' ? 'المتبقي' : 'remaining'} {line.remainingQuantity}</small></span><b>{money(line.lineTotalMinor, lang)}</b></label>
                    <div className="return-line-controls"><label>{lang === 'ar' ? 'الكمية' : 'Qty'}<input type="number" min="1" max={line.remainingQuantity || 1} step="1" value={selection.quantity} disabled={!selection.selected || workspaceBusy} onChange={(event) => setReturnSelections((current) => ({ ...current, [line.invoiceLineId]: { ...selection, quantity: event.target.value } }))} /></label><label>{lang === 'ar' ? 'حالة القطعة' : 'Condition'}<select value={selection.itemCondition} disabled={!selection.selected || workspaceBusy} onChange={(event) => setReturnSelections((current) => ({ ...current, [line.invoiceLineId]: { ...selection, itemCondition: event.target.value } }))}><option value="NEW_TAGS">{lang === 'ar' ? 'جديدة بالبطاقة' : 'New with tags'}</option><option value="UNOPENED">{lang === 'ar' ? 'غير مفتوحة' : 'Unopened'}</option><option value="WORN">{lang === 'ar' ? 'مستعملة' : 'Worn'}</option><option value="DAMAGED">{lang === 'ar' ? 'تالفة' : 'Damaged'}</option></select></label><label>{lang === 'ar' ? 'المصير' : 'Disposition'}<select value={selection.disposition} disabled={!selection.selected || workspaceBusy} onChange={(event) => setReturnSelections((current) => ({ ...current, [line.invoiceLineId]: { ...selection, disposition: event.target.value } }))}><option value="RESTOCK">{lang === 'ar' ? 'إعادة للمخزون الصالح' : 'Restock sellable'}</option><option value="DAMAGED">{lang === 'ar' ? 'مخزون تالف' : 'Damaged stock'}</option><option value="QUARANTINE">{lang === 'ar' ? 'حجر ومراجعة' : 'Quarantine'}</option><option value="DISCARD">{lang === 'ar' ? 'إتلاف / لا يعاد' : 'Discard'}</option></select></label></div>
                  </div>;
                })}{returnableInvoice.lines.length === 0 && <div className="empty-table">{lang === 'ar' ? 'لا توجد بنود قابلة للإرجاع.' : 'No returnable lines.'}</div>}</div>}</section>
              </div>
              {returnableInvoice && <div className="module-grid returns-settlement-grid">
                <section className="module-card module-form"><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'تطابق سياسة المتجر' : 'Store policy enforced'}</span><h2>{lang === 'ar' ? 'تسوية المرتجع' : 'Return settlement'}</h2></div><div className="stat-icon mint"><Icon name="tag" /></div></div><label>{lang === 'ar' ? 'سبب الإرجاع (إلزامي)' : 'Return reason (required)'}<textarea rows={3} value={returnReason} onChange={(event) => setReturnReason(event.target.value)} /></label><label>{lang === 'ar' ? 'وسيلة رد المبلغ' : 'Refund method'}<select value={refundMethod} onChange={(event) => setRefundMethod(event.target.value)}><option value="CASH">{lang === 'ar' ? 'نقدي' : 'Cash'}</option><option value="CARD">{lang === 'ar' ? 'بطاقة' : 'Card'}</option><option value="WALLET">{lang === 'ar' ? 'محفظة' : 'Wallet'}</option><option value="STORE_CREDIT">{lang === 'ar' ? 'رصيد للعميل' : 'Store credit'}</option></select></label><div className="return-total-box"><span>{lang === 'ar' ? 'قيمة البنود المختارة' : 'Selected returned value'}</span><strong>{money(returnedValueMinor, lang)}</strong></div><button className="auth-submit" type="button" disabled={workspaceBusy || !selectedReturnLines.length} onClick={() => void postReturn()}>{lang === 'ar' ? 'تسجيل المرتجع ورد قيمته' : 'Post return and refund'}<Icon name="check" size={16} /></button></section>
                <section className="module-card module-form"><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'معاملة ذرية واحدة' : 'One atomic transaction'}</span><h2>{lang === 'ar' ? 'استبدال بصنف آخر' : 'Exchange for another item'}</h2></div><div className="stat-icon lilac"><Icon name="return" /></div></div><label>{lang === 'ar' ? 'الصنف / المتغير البديل' : 'Replacement variant'}<select value={exchangeVariantId} onChange={(event) => setExchangeVariantId(event.target.value)}><option value="">—</option>{catalogItems.flatMap((product) => product.variants.map((variant) => <option key={variant.id} value={variant.id}>{variant.sku} · {lang === 'ar' ? product.nameAr : product.nameEn} · {variant.size} / {variant.colorEn} · {money(variant.priceMinor, lang)}</option>))}</select></label><label>{lang === 'ar' ? 'كمية الصنف البديل' : 'Replacement quantity'}<input type="number" min="1" max="10000" step="1" value={exchangeQuantity} onChange={(event) => setExchangeQuantity(event.target.value)} /></label><div className="return-total-box"><span>{exchangeDifference > 0 ? (lang === 'ar' ? 'المطلوب تحصيله' : 'Amount to collect') : exchangeDifference < 0 ? (lang === 'ar' ? 'المطلوب رده' : 'Amount to refund') : (lang === 'ar' ? 'فرق السعر' : 'Price difference')}</span><strong>{money(Math.abs(exchangeDifference), lang)}{exchangeDifference > 0 ? ' +' : exchangeDifference < 0 ? ' −' : ''}</strong></div><div className="module-fields-two"><label>{lang === 'ar' ? 'وسيلة التحصيل' : 'Collection method'}<select value={exchangePaymentMethod} onChange={(event) => setExchangePaymentMethod(event.target.value)}><option value="CASH">{lang === 'ar' ? 'نقدي' : 'Cash'}</option><option value="CARD">{lang === 'ar' ? 'بطاقة' : 'Card'}</option><option value="WALLET">{lang === 'ar' ? 'محفظة' : 'Wallet'}</option></select></label><label>{lang === 'ar' ? 'وسيلة الرد' : 'Refund method'}<select value={exchangeRefundMethod} onChange={(event) => setExchangeRefundMethod(event.target.value)}><option value="CASH">{lang === 'ar' ? 'نقدي' : 'Cash'}</option><option value="CARD">{lang === 'ar' ? 'بطاقة' : 'Card'}</option><option value="WALLET">{lang === 'ar' ? 'محفظة' : 'Wallet'}</option><option value="STORE_CREDIT">{lang === 'ar' ? 'رصيد للعميل' : 'Store credit'}</option></select></label></div><button className="auth-submit" type="button" disabled={workspaceBusy || !selectedReturnLines.length || !exchangeVariantId} onClick={() => void postExchange()}>{lang === 'ar' ? 'تنفيذ الاستبدال وإصدار فاتورة' : 'Complete exchange and issue invoice'}<Icon name="check" size={16} /></button></section>
              </div>}
            </>}
            {activeTab === 'reports' && <div className="module-card report-module"><div className="module-card-head"><div><span className="modal-eyebrow">{lang === 'ar' ? 'تحكم بالصلاحيات' : 'Permission-gated data'}</span><h2>{lang === 'ar' ? 'ملخص الفترة' : 'Period summary'}</h2></div><div className="stat-icon lilac"><Icon name="chart" /></div></div><div className="module-fields-two"><label>{lang === 'ar' ? 'من' : 'From'}<input type="date" value={reportRange.from} onChange={(event) => setReportRange((range) => ({ ...range, from: event.target.value }))} /></label><label>{lang === 'ar' ? 'إلى' : 'To'}<input type="date" value={reportRange.to} onChange={(event) => setReportRange((range) => ({ ...range, to: event.target.value }))} /></label></div><button className="auth-submit report-load" onClick={() => void loadReports()} disabled={workspaceBusy}>{lang === 'ar' ? 'تحميل التقرير' : 'Load report'}<Icon name="chart" size={16} /></button>{salesSummary && <div className="report-cards"><div><span>{lang === 'ar' ? 'الفواتير' : 'Invoices'}</span><strong>{salesSummary.invoiceCount}</strong></div><div><span>{lang === 'ar' ? 'الإجمالي' : 'Sales'}</span><strong>{money(salesSummary.grossMinor, lang)}</strong></div><div><span>{lang === 'ar' ? 'الخصومات' : 'Discounts'}</span><strong>{money(salesSummary.discountsMinor, lang)}</strong></div><div><span>{lang === 'ar' ? 'المرتجعات' : 'Returns'}</span><strong>{money(salesSummary.returnsMinor, lang)}</strong></div><div className="report-net"><span>{lang === 'ar' ? 'صافي المبيعات' : 'Net sales'}</span><strong>{money(salesSummary.netMinor, lang)}</strong></div><div><span>{lang === 'ar' ? 'الضريبة المسجلة' : 'Recorded tax'}</span><strong>{money(salesSummary.taxMinor, lang)}</strong></div></div>}{profitSummary && <div className="profit-strip"><div><span>{lang === 'ar' ? 'الإيراد دون الضريبة' : 'Net revenue'}</span><strong>{money(profitSummary.revenueMinor, lang)}</strong></div><div><span>{lang === 'ar' ? 'التكلفة المسجلة' : 'Recorded cost basis'}</span><strong>{money(profitSummary.costMinor, lang)}</strong></div><div><span>{lang === 'ar' ? 'مجمل الربح' : 'Gross profit'}</span><strong>{money(profitSummary.grossProfitMinor, lang)} · {(profitSummary.marginBps / 100).toFixed(1)}%</strong></div></div>}<div className="auth-helper">{lang === 'ar' ? 'التكلفة وهامش الربح لا يظهران إلا عند امتلاك صلاحية وتقنية الترخيص المناسبة. تكلفة التقرير تعتمد على تكلفة الوحدة المسجلة وقت البيع.' : 'Cost and margin are returned only with the required role and license feature. Cost reporting uses the unit cost snapshot recorded at sale time.'}</div></div>}
            <footer className="page-footer"><span><i className="footer-dot" />{isTauri ? (lang === 'ar' ? 'العمليات تحفظ محليًا' : 'Operations save locally') : t.preview}</span><span>{lang === 'ar' ? 'البيانات لا تغادر هذا الجهاز' : 'Data stays on this device'}</span><span className="footer-build">NASEEJ POS <b>0.1.0</b></span></footer>
          </section>}
        </div>
      </main>

      {activeProduct && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setActiveProduct(null); }}><section className="variant-modal" role="dialog" aria-modal="true" aria-labelledby="variant-title"><div className="modal-top"><div><span className="modal-eyebrow">{activeProduct.brand} · {lang === 'ar' ? activeProduct.categoryAr : activeProduct.categoryEn}</span><h2 id="variant-title">{lang === 'ar' ? activeProduct.nameAr : activeProduct.nameEn}</h2></div><button className="modal-close" onClick={() => setActiveProduct(null)} aria-label={t.cancel}><Icon name="close" /></button></div><div className={`modal-art ${activeProduct.art}`}><div className="garment-shape"><span /></div></div><div className="variant-price-line"><strong>{money(activeProduct.priceMinor, lang)}</strong><span>{t.chooseVariant}</span></div><div className="variant-section"><div className="variant-label"><strong>{t.size}</strong><span>{selectedSize}</span></div><div className="size-options">{[...new Set(activeProduct.variants.map((v) => v.size))].map((size) => <button key={size} className={selectedSize === size ? 'chosen' : ''} onClick={() => setSelectedSize(size)}>{size}</button>)}</div></div><div className="variant-section"><div className="variant-label"><strong>{t.color}</strong><span>{lang === 'ar' ? activeProduct.variants.find((v) => v.color === selectedColor)?.colorAr : activeProduct.variants.find((v) => v.color === selectedColor)?.colorEn}</span></div><div className="color-options">{[...new Map(activeProduct.variants.map((v) => [v.color, v])).values()].map((variant) => <button key={variant.color} className={selectedColor === variant.color ? 'chosen' : ''} onClick={() => setSelectedColor(variant.color)} title={lang === 'ar' ? variant.colorAr : variant.colorEn}><i style={{ backgroundColor: variant.colorHex }} /></button>)}</div></div><div className="variant-stock-box"><div><span>{t.stock}</span><strong>{activeProduct.variants.find((v) => v.size === selectedSize && v.color === selectedColor)?.stock ?? 0} {t.units}</strong></div><div><span>{t.sku}</span><strong className="mono">{activeProduct.variants.find((v) => v.size === selectedSize && v.color === selectedColor)?.sku ?? '—'}</strong></div></div><div className="modal-actions"><button className="modal-cancel" onClick={() => setActiveProduct(null)}>{t.cancel}</button><button className="modal-add" onClick={() => { const variant = activeProduct.variants.find((v) => v.size === selectedSize && v.color === selectedColor); if (variant) addVariant(activeProduct, variant); }}><Icon name="plus" size={17} />{t.addToCart}</button></div></section></div>}

      {customerModalOpen && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setCustomerModalOpen(false); }}><section className="variant-modal customer-modal" role="dialog" aria-modal="true" aria-labelledby="customer-title"><div className="modal-top"><div><span className="modal-eyebrow">{lang === 'ar' ? 'بيانات مشفّرة محليًا' : 'Locally encrypted customer data'}</span><h2 id="customer-title">{lang === 'ar' ? 'عميل اختياري' : 'Optional customer'}</h2></div><button className="modal-close" onClick={() => setCustomerModalOpen(false)} aria-label={t.cancel}><Icon name="close" /></button></div>
        {selectedCustomer && <div className="auth-warning">{selectedCustomer.name}{selectedCustomer.phone ? ` · ${selectedCustomer.phone}` : ''}{selectedCustomer.loyaltyPoints ? ` · ${selectedCustomer.loyaltyPoints} ${lang === 'ar' ? 'نقطة' : 'points'}` : ''}<button type="button" className="text-action" onClick={() => { setSelectedCustomer(null); setCustomerMessage(''); }}>{lang === 'ar' ? 'إزالة الاختيار' : 'Clear selection'}</button></div>}
        <form className="auth-form" onSubmit={lookupCustomer}><label>{lang === 'ar' ? 'بحث برقم الهاتف' : 'Find by phone'}<input type="tel" value={customerForm.phone} onChange={(event) => setCustomerForm((form) => ({ ...form, phone: event.target.value }))} required /></label><button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'بحث' : 'Search'}<Icon name="search" size={16} /></button></form>
        <div className="section-line"><div><h2>{lang === 'ar' ? 'أو أضف عميلاً' : 'Or add a customer'}</h2></div></div>
        <form className="auth-form" onSubmit={createCustomer}><label>{lang === 'ar' ? 'الاسم' : 'Name'}<input value={customerForm.name} onChange={(event) => setCustomerForm((form) => ({ ...form, name: event.target.value }))} required /></label><label>{lang === 'ar' ? 'البريد الإلكتروني (اختياري)' : 'Email (optional)'}<input type="email" value={customerForm.email} onChange={(event) => setCustomerForm((form) => ({ ...form, email: event.target.value }))} /></label><label>{lang === 'ar' ? 'حالة الموافقة على التسويق' : 'Marketing consent'}<select value={customerForm.consentStatus} onChange={(event) => setCustomerForm((form) => ({ ...form, consentStatus: event.target.value }))}><option value="NOT_GIVEN">{lang === 'ar' ? 'لم تُمنح' : 'Not given'}</option><option value="GIVEN">{lang === 'ar' ? 'ممنوحة' : 'Given'}</option></select></label>{customerMessage && <div className="auth-helper">{customerMessage}</div>}<button className="auth-submit" type="submit" disabled={workspaceBusy}>{lang === 'ar' ? 'إنشاء واختيار' : 'Create and select'}<Icon name="check" size={16} /></button></form>
        {selectedCustomer && <button className="modal-add customer-use" onClick={() => setCustomerModalOpen(false)}>{lang === 'ar' ? 'استخدام في الفاتورة' : 'Use on this sale'}<Icon name="check" size={16} /></button>}
      </section></div>}

      {notice && <div className={`toast ${notice.kind}`} role="status"><span className="toast-symbol">{notice.kind === 'success' ? <Icon name="check" size={17} /> : '!'}</span><span>{notice.text}</span><button onClick={() => setNotice(null)} aria-label="إغلاق"><Icon name="close" size={14} /></button></div>}
      {!isTauri && <div className="preview-ribbon"><span className="preview-spark">✦</span>{t.preview}</div>}
      {ownerPanelOpen && <OwnerPanel lang={lang} onClose={() => setOwnerPanelOpen(false)} />}
    </div>
  );
}

export default App;
