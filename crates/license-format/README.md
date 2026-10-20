# `naseej-license-format`

مكتبة مشتركة لمخطط `.req/.lic`، serialization ثابت للـ payload، التحقق Ed25519، وبصمة الجهاز. تحتوي واجهات العميل على دوال **verification فقط**؛ لا يوجد فيها توقيع أو seed خاص.

## المخططات والخطط

- `.req` يستخدم `ACTIVATION_REQUEST_SCHEMA_VERSION = 1`.
- ملفات الترخيص الجديدة تستخدم schema version 2 وتضم `plan_code`: `trial_7d` أو `annual_12m`. يقبل verifier ملفات v1 الموقعة التي لم يكن بها `plan_code` للتوافق.
- التوقيع على `serde_json::to_vec(&LicensePayload)` بترتيب struct ثابت؛ يجب ترقية schema وإضافة اختبارات توافق عند تغيير الحقول أو التطبيع.
- الخطة تحددها أداة الإصدار. يحدد issuer حاليًا التجربة بـ 7 أيام والسنوي بـ 12 شهرًا تقويميًا؛ لا تحتوي المكتبة على تسعير أو تجديد آلي.

## بناء العميل

قبل بناء POS/ActivationTool للإنتاج، يمرر build pipeline `NASEEJ_LICENSE_PUBLIC_KEY_HEX` (64 hex chars، المفتاح العام المطابق لمفتاح issuer). غيابه يسبب رفضًا واضحًا ولا يوجد fallback يقبل أي ملف.
