# سجل إصدار المقترحات التقنية

تاريخ التقييم: 2026-10-10 (إصدارات الإنتاج تُثبت فعليًا في lockfiles قبل البناء).

- Tauri stable 2.12.x وJS API/CLI 2.12.1.
- React/React DOM 19.3.0؛ TypeScript 6.0.3.
- Vite 8.3.2 مع Node 20.19+ أو 22.12+.
- rusqlite 0.40.2؛ feature `bundled-sqlcipher-vendored-openssl`؛ changelog يذكر SQLCipher 4.14.0.
- Argon2 crate 0.6.x، Ed25519-dalek 3.x.

يُراجع التوافق الأمني لكل حزمة خلال التنفيذ، ويُنشأ SBOM/Cargo.lock/package-lock.json موحد؛ هذه ليست قائمة إصدارات مقفلة أو ضمانًا تلقائيًا لعدم وجود ثغرات.