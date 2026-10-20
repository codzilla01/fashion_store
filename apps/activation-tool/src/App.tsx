import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

type DeviceInfo = { fingerprint: string; algorithm: string };
type ActivationRequest = { schema_version: number; request_id: string; product: string; app_version: string; requested_at_utc: string; device_fingerprint: string };
type ActivationStatus = { licenseId: string; customerId: string; planCode: string | null; expiresAtUtc: string; features: string[]; valid: boolean };

function Icon({ type }: { type: 'key' | 'cpu' | 'file' | 'check' | 'download' | 'upload' | 'shield' | 'clock' | 'alert' }) {
  const paths = {
    key: 'M21 2 19 4m-7 7a5 5 0 1 1-7-7 5 5 0 0 1 7 7Zm0 0L21 20m-5-5 3 3m-5-5 3 3',
    cpu: 'M9 9h6v6H9zM4 9h5M4 15h5m6-6h5m-5 6h5M9 4v5m6-5v5m-6 6v5m6-5v5',
    file: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Zm0 0v6h6M8 17h8M8 13h2',
    check: 'm5 12 4 4L19 6',
    download: 'M12 3v12m-5-5 5 5 5-5M4 20h16',
    upload: 'M12 15V3m-5 5 5-5 5 5M4 20h16',
    shield: 'M12 22s8-4 8-11V5l-8-3-8 3v6c0 7 8 11 8 11Zm-3-10 2 2 4-4',
    clock: 'M12 7v5l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z',
    alert: 'M12 9v4m0 4h.01M10.3 3.9 1.8 18.2A2 2 0 0 0 3.5 21h17a2 2 0 0 0 1.7-2.8L13.7 3.9a2 2 0 0 0-3.4 0Z',
  };
  return <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[type]} /></svg>;
}

function App() {
  const native = Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
  const [lang, setLang] = useState<'ar' | 'en'>('ar');
  const [device, setDevice] = useState<DeviceInfo | null>(null);
  const [activationRequest, setActivationRequest] = useState<ActivationRequest | null>(null);
  const [status, setStatus] = useState<ActivationStatus | null>(null);
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const ar = lang === 'ar';

  useEffect(() => {
    if (!native) return;
    Promise.all([
      invoke<DeviceInfo>('get_device_info'),
      invoke<ActivationStatus | null>('get_activation_status'),
    ]).then(([info, current]) => { setDevice(info); setStatus(current); }).catch(() => setMessage({ text: ar ? 'تعذر قراءة حالة التفعيل أو بصمة الجهاز.' : 'Could not read activation or device state.', error: true }));
  }, [native, ar]);

  async function createRequest() {
    if (!native) {
      const demo: ActivationRequest = { schema_version: 1, request_id: crypto.randomUUID(), product: 'naseej-pos', app_version: '0.1.0', requested_at_utc: new Date().toISOString(), device_fingerprint: 'preview-only-no-hardware-id' };
      setActivationRequest(demo);
      setMessage({ text: ar ? 'هذه معاينة؛ بصمة الجهاز في المتصفح تجريبية فقط.' : 'Preview only; browser fingerprint is demo data.', error: false });
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const request = await invoke<ActivationRequest>('create_activation_request');
      setActivationRequest(request);
      setDevice({ fingerprint: request.device_fingerprint, algorithm: 'SHA-256(SMBIOS System UUID, v1)' });
      setMessage({ text: ar ? 'تم إنشاء طلب التفعيل محليًا.' : 'Activation request created locally.', error: false });
    } catch {
      setMessage({ text: ar ? 'تعذر إنشاء الطلب. قد لا يدعم الجهاز قراءة UUID للوحة الأم؛ تواصل مع الدعم.' : 'Could not create a request. This device may not expose an SMBIOS UUID; contact support.', error: true });
    } finally { setBusy(false); }
  }

  function downloadRequest() {
    if (!activationRequest) return;
    const blob = new Blob([JSON.stringify(activationRequest, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `${activationRequest.request_id}.req`;
    link.click();
    URL.revokeObjectURL(url);
  }

  async function importLicense() {
    if (!file) { fileRef.current?.click(); return; }
    setBusy(true);
    setMessage(null);
    try {
      const licenseJson = await file.text();
      if (!native) {
        setMessage({ text: ar ? 'المعاينة لا تتحقق من توقيع Ed25519 أو تحفظ الرخصة.' : 'Preview does not verify Ed25519 signatures or store a license.', error: true });
        return;
      }
      const result = await invoke<ActivationStatus>('import_activation_license', { licenseJson });
      setStatus(result);
      setFile(null);
      if (fileRef.current) fileRef.current.value = '';
      setMessage({ text: ar ? 'تم التحقق من التوقيع وحفظ الرخصة محليًا.' : 'Signature verified and license stored locally.', error: false });
    } catch {
      setMessage({ text: ar ? 'رُفضت الرخصة: الملف/التوقيع/المنتج/الجهاز/التاريخ غير صالح. لم تُستبدل الرخصة الحالية.' : 'License rejected: file, signature, product, device, or dates are invalid. The current license was not replaced.', error: true });
    } finally { setBusy(false); }
  }

  async function exportLicenseBackup() {
    if (!native) return;
    try {
      const contents = await invoke<string>('export_license_backup');
      const blob = new Blob([contents], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = 'naseej-license-backup.lic';
      link.click();
      URL.revokeObjectURL(url);
      setMessage({ text: ar ? 'تم تنزيل نسخة احتياطية من ملف الرخصة.' : 'License backup downloaded.', error: false });
    } catch {
      setMessage({ text: ar ? 'لا توجد رخصة صالحة لتصديرها.' : 'There is no license to export.', error: true });
    }
  }

  const expiryText = status?.expiresAtUtc ? new Intl.DateTimeFormat(ar ? 'ar-EG' : 'en-GB', { dateStyle: 'long' }).format(new Date(status.expiresAtUtc)) : '—';
  const planText = status?.planCode === 'trial_7d'
    ? (ar ? 'تجربة 7 أيام' : '7-day trial')
    : status?.planCode === 'annual_12m'
      ? (ar ? 'اشتراك سنوي' : 'Annual subscription')
      : (ar ? 'ترخيص سابق' : 'Legacy license');

  return (
    <div className="activation-shell" dir={ar ? 'rtl' : 'ltr'}>
      <header className="activation-topbar">
        <div className="activation-brand"><div className="activation-mark">ن</div><div><strong>نسيج</strong><span>{ar ? 'أداة إدارة التفعيل' : 'Activation manager'}</span></div></div>
        <div className="activation-top-actions"><span className="offline-badge"><i />{ar ? 'يعمل دون اتصال' : 'Offline capable'}</span><button onClick={() => setLang(ar ? 'en' : 'ar')}>{ar ? 'EN' : 'العربية'}</button></div>
      </header>
      <main className="activation-content">
        <div className="activation-heading"><div><span className="activation-kicker">{ar ? 'ترخيص الجهاز' : 'Device licensing'}</span><h1>{ar ? 'تفعيل نظام نقاط البيع' : 'Activate your POS'}</h1><p>{ar ? 'هذه الأداة مستقلة؛ يمكنك فتحها وإنشاء طلب حتى لو لم يكن POS مفعّلًا.' : 'This standalone utility can create a request even when POS is not activated.'}</p></div><div className="shield-illustration"><Icon type="shield" /></div></div>
        <section className="license-status-card"><div className="license-status-icon"><Icon type={status?.valid ? 'check' : 'key'} /></div><div className="license-status-copy"><span>{ar ? 'حالة الرخصة على هذا الجهاز' : 'License status on this device'}</span><strong>{status?.valid ? (ar ? 'مفعّلة' : 'Active') : (ar ? 'لا توجد رخصة مستوردة' : 'No license imported')}</strong>{status?.valid && <small>{planText} · {ar ? `تنتهي في ${expiryText}` : `Expires ${expiryText}`}</small>}</div><div className={`license-state-pill ${status?.valid ? 'valid' : ''}`}><i />{status?.valid ? (ar ? 'صالحة' : 'Valid') : (ar ? 'بانتظار التفعيل' : 'Awaiting activation')}</div></section>

        <div className="activation-columns">
          <section className="activation-card request-card"><div className="step-label"><span>01</span><span>{ar ? 'طلب تفعيل' : 'Activation request'}</span></div><div className="step-icon request-icon"><Icon type="cpu" /></div><h2>{ar ? 'اربط الرخصة بهذا الجهاز' : 'Bind a license to this device'}</h2><p>{ar ? 'أنشئ ملف طلب يحتوي معرّف طلب عشوائيًا وبصمة جهاز مجزّأة فقط. لا تُرسل أسماء العملاء أو أرقام هواتفهم.' : 'Create a request containing a random request ID and a hashed device fingerprint only. No customer names or phone numbers are included.'}</p>
            <div className="device-summary"><div className="device-summary-icon"><Icon type="cpu" /></div><div><span>{ar ? 'بصمة الجهاز' : 'Device fingerprint'}</span><code>{device?.fingerprint ? `${device.fingerprint.slice(0, 12)}••••••••••••••••••••••••` : (native ? (ar ? 'جارٍ القراءة…' : 'Reading…') : 'preview-only')}</code></div><span className="device-lock"><Icon type="shield" /></span></div>
            {activationRequest && <div className="request-generated"><span className="request-check"><Icon type="check" /></span><div><strong>{ar ? 'الطلب جاهز' : 'Request ready'}</strong><small>{activationRequest.request_id}</small></div><button onClick={downloadRequest}><Icon type="download" />{ar ? 'تنزيل .req' : 'Download .req'}</button></div>}
            <button className="primary-activation-button" onClick={createRequest} disabled={busy}><Icon type="file" />{busy ? '…' : activationRequest ? (ar ? 'إنشاء طلب جديد' : 'Create a new request') : (ar ? 'إنشاء طلب تفعيل' : 'Create activation request')}</button>
          </section>

          <section className="activation-card import-card"><div className="step-label"><span>02</span><span>{ar ? 'استيراد الرخصة' : 'Import license'}</span></div><div className="step-icon import-icon"><Icon type="key" /></div><h2>{ar ? 'استورد ملف الرخصة الموقّع' : 'Import the signed license'}</h2><p>{ar ? 'بعد إرسال ملف .req للجهة المرخِّصة واستلام .lic، تتحقق الأداة من التوقيع والجهاز والتاريخ محليًا دون إنترنت.' : 'After the issuer returns a .lic file, this tool verifies signature, device, and dates locally — no internet required.'}</p>
            <input ref={fileRef} className="hidden-file" type="file" accept=".lic,application/json" onChange={(event) => setFile(event.target.files?.[0] ?? null)} />
            <button className={`drop-zone ${file ? 'has-file' : ''}`} onClick={() => fileRef.current?.click()}><span className="drop-icon"><Icon type={file ? 'check' : 'upload'} /></span><strong>{file ? file.name : (ar ? 'اختر ملف .lic من هذا الجهاز' : 'Choose a .lic file from this device')}</strong><small>{file ? (ar ? 'جاهز للتحقق' : 'Ready to verify') : (ar ? 'لا حاجة إلى اتصال إنترنت' : 'No internet connection required')}</small></button>
            <button className="secondary-activation-button" onClick={importLicense} disabled={busy}><Icon type="shield" />{busy ? '…' : (ar ? 'تحقق واستيراد' : 'Verify and import')}</button>
            {status?.valid && <button className="backup-link" onClick={exportLicenseBackup}><Icon type="download" />{ar ? 'حفظ نسخة احتياطية من .lic' : 'Back up .lic license'}</button>}
          </section>
        </div>

        <section className="activation-help"><div className="help-title"><span className="help-icon"><Icon type="clock" /></span><div><strong>{ar ? 'كيف يعمل التفعيل الأوفلاين؟' : 'How offline activation works'}</strong><span>{ar ? 'ثلاث خطوات — ولا يتصل برنامج البيع بالإنترنت' : 'Three steps — the POS never needs internet'}</span></div></div><div className="help-steps"><div><b>1</b><span>{ar ? 'أنشئ ملف الطلب .req' : 'Create a .req request'}</span></div><i /><div><b>2</b><span>{ar ? 'أرسله للجهة المرخِّصة' : 'Send it to the issuer'}</span></div><i /><div><b>3</b><span>{ar ? 'استورد ملف .lic الموقّع' : 'Import the signed .lic'}</span></div></div></section>
        <footer className="activation-footer"><span><Icon type="shield" />{ar ? 'المفتاح الخاص للتوقيع لا يوجد في هذه الأداة.' : 'The signing private key is not in this utility.'}</span><span>ActivationTool · 0.1.0</span></footer>
      </main>
      {message && <div className={`activation-toast ${message.error ? 'error' : ''}`}><Icon type={message.error ? 'alert' : 'check'} />{message.text}<button onClick={() => setMessage(null)}>×</button></div>}
    </div>
  );
}

export default App;
