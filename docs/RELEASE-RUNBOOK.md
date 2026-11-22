# دليل إصدار النسخة التجارية

هذا الدليل يشرح كيف تُنتج نسخة موقّعة من متجر ملابس واحد. لا تُوزَّع أي
نسخة قبل إكمال كل خطوة وإرفاق بصمة SHA-256 أو نسخة معلومة من المستودع.

## 1. الأسرار المطلوبة في GitHub

أضف الأسرار من: `Settings → Secrets and variables → Actions → New repository secret`.
**لا تكتب قيمها في ملفات المستودع ولا في المحادثة.**

| الاسم | المطلوب | الشكل |
|---|---|---|
| `NASEEJ_LICENSE_PUBLIC_KEY_HEX` | نعم | 64 حرفًا سداسيًا للمفتاح العام Ed25519 للإنتاج |
| `CODE_SIGNING_PFX_BASE64` | نعم | محتوى ملف `.pfx` بترميز Base64 سطر واحد |
| `CODE_SIGNING_PFX_PASSWORD` | نعم | كلمة مرور الـ PFX |

> لا يوجد مفتاح إنتاجي أو شهادة توقيع داخل المستودع، وهذا مقصود. مسار
> `release.yml` يفشل بوضوح إذا كانت القيم مفقودة أو كانت حقل المفتاح أصفارًا.

## 2. توليد مفتاح التوقيع (على جهاز معزول)

```powershell
# على جهاز إصدار منفصل، دون اتصال بالإنترنت
$seed = New-Object byte[] 32
[Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($seed)
# احفظ الـ seed في HSM أو خزنة مربوطة بـ DPAPI، ولا ترفعه إلى Git
```

المفتاح العام المقابل يُحقن كـ secret أعلاه. المفتاح الخاص يبقى في أداة
الإصدار `tools/license-issuer` أو لوحة Super Admin على جهاز المطور فقط.

## 3. السياسات واختبار الأجهزة

1. راجع `docs/RELEASE-DECISIONS.md`؛ يجب أن يحتوي على `POLICY_STATUS: APPROVED`.
2. نفّذ `docs/ACCEPTANCE-RECORD.md` بالكامل على Windows حقيقي مع الطابعة
   والماسح، ثم غيّر السطر إلى `ACCEPTANCE_STATUS: PASS`.
3. تأكد أن رقم الإصدار متطابق في `package.json` و`tauri.conf.json` للتطبيقين.

## 4. تشغيل الإصدار

من GitHub Actions اختر **Production release → Run workflow** وأدخل رقم
الإصدار، أو ادفع وسمًا:

```bash
git tag v0.1.0
git push origin v0.1.0
```

ال workflow ينفّذ تلقائيًا:

1. فحص تطابق الإصدار، وجود المفتاح الإنتاجي، شهادة التوقيع، السياسات المعتمدة، وسجل القبول.
2. توليد `sbom.json` بصيغة CycloneDX.
3. بناء مثبتي POS وActivation Tool بمفتاح التوقيع الحقيقي.
4. توقيع كلا المثبتين بـ SHA-256 مع ختم زمني، والتحقق من التوقيع.
5. إنشاء `SHA256SUMS.txt` و`RELEASE_NOTES.md`، ونشر GitHub Release مع المثبتات والـ SBOM و`RIGHTS.txt`.

## 5. التحقق عند الاستلام

```powershell
Get-FileHash .\Naseej POS_0.1.0_x64-setup.exe -Algorithm SHA256
Get-AuthenticodeSignature .\Naseej POS_0.1.0_x64-setup.exe
```

قارن البصمة بملف `SHA256SUMS.txt`، وتأكد أن حالة التوقيع `Valid` وأن سلسلة
الشهادات تنتهي بـ CA موثوق. لا تُدخل بيانات متجر حقيقية قبل ذلك.

## 6. ما لا يدخل حزمة العميل

- `tools/license-issuer`
- `apps/super-admin`
- أي `.req` أو `.lic` أو `.bkz` خاص بمحل آخر
- أي مفتاح خاص أو كلمة مرور

---

## 7. إضافة الأسرار خطوة بخطوة (دليل عملي)

### 7.1 مكان الإضافة

GitHub → المستودع `codzilla01/fashion_store` → **Settings** →
**Secrets and variables** → **Actions** → زر **New repository secret**.

أضف الأسرار الثلاثة بأسمائها الحرفية تمامًا (حساسة لحالة الأحرف):

| الاسم | القيمة |
|---|---|
| `NASEEJ_LICENSE_PUBLIC_KEY_HEX` | مفتاح التحقق العام Ed25519، 64 حرفًا سداسيًا |
| `CODE_SIGNING_PFX_BASE64` | ملف `.pfx` بترميز Base64 في سطر واحد |
| `CODE_SIGNING_PFX_PASSWORD` | كلمة مرور الـ`.pfx` |

> لا تُرسل القيم في المحادثة ولا تضعها في ملف داخل المستودع.
> لا يطبع مسار الإصدار أي قيمة؛ يطبع فقط «configured».

### 7.2 المفتاح العام للرخصة (على جهاز الإصدار المعزول)

المفتاح العام **مشتق** من ملف الـseed ولا يُختار يدويًا:

```powershell
# 1) توليد seed عشوائي 32 بايت (مرة واحدة فقط لكل منتج)
$seed = New-Object byte[] 32
[Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($seed)
[IO.File]::WriteAllBytes("$env:USERPROFILE\naseej-prod-seed.bin", $seed)

# 2) اشتقاق المفتاح العام من نفس الملف
cargo run --release --manifest-path tools\license-issuer\Cargo.toml -- `
  public-key --private-key-file "$env:USERPROFILE\naseej-prod-seed.bin"
# يطبع: NASEEJ_LICENSE_PUBLIC_KEY_HEX=<64 hex>
```

انسخ الـ64 حرفًا فقط (بدون `NASEEJ_LICENSE_PUBLIC_KEY_HEX=`) إلى السر.
احتفظ بملف الـseed في خزنة/HSM منفصلة، وفقدانه يعني استحالة إصدار رخص
تطابق النسخ الموزّعة.

### 7.3 شهادة توقيع الكود (Code Signing)

تحتاج شهادة **Code Signing** صالحة (OV أو EV) من CA موثوق، بصيغة `.pfx`
(صادر من جهة تُصدر شهادات Windows). ثم:

```powershell
# تحويل الملف إلى Base64 في سطر واحد (بدون أسطر جديدة)
$b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes("C:\secure\naseej-codesign.pfx"))
Set-Clipboard -Value $b64   # الصقها في CODE_SIGNING_PFX_BASE64
```

- الصق كلمة المرور في `CODE_SIGNING_PFX_PASSWORD`.
- المسار يوقّع بـ`signtool` مع ختم زمني `http://timestamp.digicert.com`
  وخوارزمية SHA-256، ثم يتحقق بـ`signtool verify /pa` قبل النشر.
- الحد الأدنى المقبول في الفحص: Base64 صالح، وحجم ≥1024 بايت، ويبدأ بـ`0x30`
  (بداية PKCS#12).

### 7.4 التحقق قبل الإصدار

1. من **Actions** → **Production release** → **Run workflow** بالنسخة `0.1.0`.
2. ستنجح خطوة `Release gates` فقط إذا تحققت كل البوابات:
   - الإصدار متطابق في `package.json` و`tauri.conf.json`.
   - المفتاح 64 حرفًا سداسيًا وليس أصفارًا.
   - الـPFX Base64 صالح وكلمة المروره موجودة.
   - `POLICY_STATUS: APPROVED` و`ACCEPTANCE_STATUS: PASS`.

> `ACCEPTANCE_STATUS` ما زال `PENDING` حاليًا، لذلك سيتوقف المسار عند هذه
> البوابة حتى تُنفّذ اختبارات الأجهزة وتُوقّع في `docs/ACCEPTANCE-RECORD.md`.

### 7.5 عند وجود عدة مستودعات/بيئات

- **Environment secrets** (Settings → Environments → production) أفضل من
  Repository secrets: تسمح بقاعدة موافقة يدوية قبل النشر. يجب بعدها إضافة
  `environment: production` إلى وظيفة `build-and-publish` في `release.yml`.
- **Organization secrets** لا تُستخدم هنا؛ المشروع مستودع واحد.

### 7.6 تنظيف وتدوير

- عند تسريب الـseed أو الـPFX: أنشئ مفتاحًا/شهادة جديدة، استبدل السر، وأصدر
  نسخة جديدة؛ النسخ القديمة لا تُصلَح بأثر رجعي.
- تُسجَّل الشهادات بتاريخ انتهاء؛ تحقق منه قبل كل إصدار سنوي.
