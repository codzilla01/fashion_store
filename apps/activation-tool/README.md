# ActivationTool — تطبيق مستقل

هذا مشروع Tauri مستقل عن `apps/pos-desktop`، له `identifier` و`Cargo.toml` و`package.json` ونافذة ومثبت منفصل؛ يستطيع المستخدم تشغيله حتى عندما لا يفتح POS أو تكون رخصته غير مفعلة. لا يقرأ قاعدة البيانات ولا يضمّن مفتاح توقيع خاصًا.

## تدفق المستخدم

1. يشغّل المستخدم `ActivationTool.exe`.
2. ينشئ `.req`؛ يتضمن product/schema/request UUID وبصمة SHA-256 مشتقة من SMBIOS System UUID فقط، لا MAC ولا serial خام ولا بيانات عميل.
3. يرسل `.req` لمسؤول الإصدار الداخلي بالطريقة المتفق عليها، ويتلقى `.lic` موقّعًا. أداة الإدارة الداخلية تدعم تجربة 7 أيام واشتراكًا سنويًا لمدة 12 شهرًا تقويميًا؛ لا يوجد إصدار تلقائي داخل تطبيق العميل.
4. يختار `.lic`؛ البرنامج يتحقق من Ed25519 والمنتج والجهاز والصلاحية محليًا، ويعرض نوع الخطة وتاريخ الانتهاء، ثم يحفظ envelope بعد حمايتها بـ DPAPI في `%LOCALAPPDATA%\NaseejPOS\Licensing\license.state.dpapi` ليستطيع POS قراءتها تحت حساب Windows نفسه.
5. يستطيع حفظ نسخة `.lic` خارجية. عند تغيير الجهاز ينشئ طلبًا جديدًا ويطلب rehost من الجهة المرخِّصة؛ الملف القديم لا يطابق البصمة الجديدة.

## بناء مستقل

```powershell
cd apps/activation-tool
npm install
$env:NASEEJ_LICENSE_PUBLIC_KEY_HEX = '<32-byte Ed25519 public key as 64 hex characters>'
npm run tauri build
```

متغير البناء أعلاه يحتوي **المفتاح العام فقط**. لا تضع private key في هذا المشروع. إن لم يُمرر public key، تعرض الأداة `PublicKeyNotConfigured` وترفض الاستيراد بدل قبول ملف دون تحقق.

أداة الإصدار الداخلية موجودة في `tools/license-issuer` وليست dependency/target للمثبت؛ راجع `tools/license-issuer/README.md`.
