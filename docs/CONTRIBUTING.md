# قواعد العمل الهندسي

- لا تكتب أسرارًا/PII/مبالغ بطاقات في console أو audit details.
- كل عملية مالية/مخزنية لها transaction، permission في Rust، idempotency key، واختبار failure path.
- لا تستخدم floating point في الأسعار أو الضرائب.
- لا تفتح قاعدة البيانات إلا من native Rust connection بعد تهيئة SQLCipher والتحقق من cipher version.
- أي مهاجرة schema لها رقم/Checksum ونسخة احتياطية قبل إصدارها؛ لا تعدل migration منشورة.
- لا يحذف فاتورة/دفعة/حركة؛ استخدم reversal موثقًا.
- تغييرات مفاتيح الترخيص لا تسمح بوجود private key في تطبيق عميل أو artifact.
- افتح نسخة `.bkz` على جهاز نظيف دوريًا، واختبر الطابعة على موديل حقيقي.
