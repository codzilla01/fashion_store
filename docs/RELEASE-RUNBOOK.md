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
