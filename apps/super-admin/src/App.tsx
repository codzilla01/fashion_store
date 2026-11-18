import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

type VaultStatus = {
  exists: boolean;
  unlocked: boolean;
  publicKeyHex: string | null;
  registryDir: string;
  appDataDir: string;
};

type SubscriptionRow = {
  licenseId: string;
  requestId: string;
  customerId: string;
  planCode: string;
  issuedAtUtc: string;
  notBeforeUtc: string;
  expiresAtUtc: string;
  deviceFingerprint: string;
  licenseFile: string;
  features: string[];
  limits: Record<string, number>;
  status: string;
  revoked: boolean;
};

type FeatureCatalog = {
  features: string[];
  plans: Array<{ code: string; label: string; days: number }>;
};

type IssueResult = {
  licenseId: string;
  customerId: string;
  planCode: string;
  planLabel: string;
  issuedAtUtc: string;
  expiresAtUtc: string;
  licensePath: string;
  licenseContent: string;
  warnings: string[];
};

type Tab = 'vault' | 'subscriptions' | 'issue';

const text = {
  title: 'لوحة نسيج الإدارية',
  subtitle: 'إدارة اشتراكات العملاء وتفعيل نسخهم',
  internal: 'أداة داخلية للمطور — لا تُوزَّع على العملاء',
  vault: 'الخزنة',
  subscriptions: 'الاشتراكات',
  issue: 'إصدار رخصة',
  vaultLocked: 'الخزنة مقفلة',
  vaultOpen: 'الخزنة مفتوحة',
  publicKey: 'المفتاح العام (يدخل في بناء تطبيق العميل)',
  createVault: 'إنشاء خزنة جديدة',
  importSeed: 'استيراد مفتاح توقيع موجود',
  unlock: 'فتح الخزنة',
  lock: 'قفل الخزنة',
  passphrase: 'كلمة مرور الخزنة',
  passphraseHint: '12 حرفًا على الأقل. لا تُفقدها: بدونها لا يمكن فك التوقيع.',
  confirmPassphrase: 'تأكيد كلمة المرور',
  seedHex: 'مفتاح التوقيع (64 محرفًا hex)',
  generated: 'تم إنشاء الخزنة',
  unlocked: 'تم فتح الخزنة',
  locked: 'تم قفل الخزنة',
  registry: 'مجلد السجل',
  customer: 'معرّف العميل',
  plan: 'الخطة',
  features: 'الميزات الممنوحة',
  maxUsers: 'أقصى عدد مستخدمين',
  maxBranches: 'أقصى عدد فروع',
  requestFile: 'ملف طلب التفعيل (.req / .json)',
  requestHint: 'الملف الذي أرسله العميل من أداة التفعيل.',
  issueButton: 'إصدار الاشتراك',
  issued: 'تم إصدار الرخصة',
  licenseFile: 'ملف الرخصة',
  copy: 'نسخ محتوى الرخصة',
  copied: 'تم النسخ',
  revoke: 'إلغاء الاشتراك',
  revoked: 'تم تسجيل الإلغاء',
  read: 'عرض الرخصة',
  customerFilter: 'بحث بمعرّف العميل',
  noSubscriptions: 'لم يُصدر أي اشتراك من هذا الجهاز بعد.',
  noResults: 'لا توجد نتائج مطابقة.',
  vaultWarning: 'مفتاح التوقيع لا يغادر هذا الجهاز. تطبيق العميل يحتوي على المفتاح العام فقط.',
  vaultDpapi: 'الخزنة مربوطة بحساب Windows لهذا الجهاز: نسخ ملف الخزنة إلى جهاز أو حساب آخر لا يفتحها.',
  offlineNote: 'الإصدار أوفلاين: لا يمكن إلغاء رicense وصلت إلى جهاز غير متصل.',
};

const errorText: Record<string, string> = {
  VAULT_ALREADY_EXISTS: 'الخزنة موجودة بالفعل. افتحها بدل إنشاء واحدة جديدة.',
  VAULT_LOCKED: 'افتح الخزنة قبل الإصدار.',
  VAULT_PASSPHRASE_WRONG: 'كلمة المرور غير صحيحة.',
  VAULT_FILE_MISSING: 'لا يوجد ملف خزنة على هذا الجهاز.',
  VAULT_FILE_INVALID: 'ملف الخزنة غير صالح.',
  VAULT_VERSION_UNSUPPORTED: 'إصدار ملف الخزنة غير مدعوم.',
  VAULT_DPAPI_UNAVAILABLE: 'هذه الخزنة تحتاج Windows DPAPI. استخدم لوحة المطور على Windows.',
  VAULT_DPAPI_PROTECT_FAILED: 'تعذر تغليف المفتاح بـWindows DPAPI.',
  VAULT_DPAPI_UNPROTECT_FAILED: 'تعذر فك تغليف المفتاح بـWindows DPAPI. قد يكون الملف من جهاز أو حساب آخر.',
  VAULT_USER_MISMATCH: 'الخزنة تخصّ حساب Windows آخر. لا يمكن فتحها على هذا الحساب.',
  REQUEST_INVALID: 'طلب التفعيل غير صالح.',
  PLAN_UNKNOWN: 'الخطة غير معروفة.',
  LICENSE_NOT_FOUND: 'الرخصة غير موجودة في السجل.',
  LICENSE_FILE_MISSING: 'ملف الرخصة غير موجود.',
};

function describe(raw: unknown): string {
  const code = String(raw).replace(/^Error:\s*/, '').split('\n')[0];
  return errorText[code] ?? code;
}

function shortFingerprint(value: string): string {
  return value.length > 18 ? `${value.slice(0, 18)}…` : value;
}

export default function App() {
  const [tab, setTab] = useState<Tab>('vault');
  const [vault, setVault] = useState<VaultStatus | null>(null);
  const [catalog, setCatalog] = useState<FeatureCatalog | null>(null);
  const [rows, setRows] = useState<SubscriptionRow[]>([]);
  const [passphrase, setPassphrase] = useState('');
  const [confirmPassphrase, setConfirmPassphrase] = useState('');
  const [seedHex, setSeedHex] = useState('');
  const [customerId, setCustomerId] = useState('');
  const [planCode, setPlanCode] = useState('trial_7d');
  const [features, setFeatures] = useState<string[]>(['pos.sell', 'inventory.manage', 'returns.exchange', 'reports.basic', 'backup.restore']);
  const [maxUsers, setMaxUsers] = useState('3');
  const [maxBranches, setMaxBranches] = useState('1');
  const [requestText, setRequestText] = useState('');
  const [requestName, setRequestName] = useState('');
  const [result, setResult] = useState<IssueResult | null>(null);
  const [notice, setNotice] = useState<{ kind: 'ok' | 'bad' | 'warn'; body: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useState('');

  const refreshVault = useCallback(async () => {
    try {
      setVault(await invoke<VaultStatus>('vault_status'));
    } catch (failure) {
      setNotice({ kind: 'bad', body: describe(failure) });
    }
  }, []);

  const refreshRows = useCallback(async () => {
    try {
      setRows(await invoke<SubscriptionRow[]>('list_subscriptions'));
    } catch (failure) {
      setNotice({ kind: 'bad', body: describe(failure) });
    }
  }, []);

  useEffect(() => {
    void refreshVault();
    void refreshRows();
    void invoke<FeatureCatalog>('feature_catalog').then(setCatalog).catch(() => undefined);
  }, [refreshRows, refreshVault]);

  const filtered = useMemo(
    () => rows.filter((row) => row.customerId.toLowerCase().includes(filter.trim().toLowerCase())),
    [filter, rows],
  );

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setNotice(null);
    try {
      await action();
    } catch (failure) {
      setNotice({ kind: 'bad', body: describe(failure) });
    } finally {
      setBusy(false);
    }
  }

  async function createVault() {
    if (passphrase !== confirmPassphrase) {
      setNotice({ kind: 'bad', body: 'كلمتا المرور غير متطابقتين.' });
      return;
    }
    await run(async () => {
      const next = await invoke<VaultStatus>('vault_create', { passphrase });
      setVault(next);
      setPassphrase('');
      setConfirmPassphrase('');
      setNotice({ kind: 'ok', body: text.generated });
    });
  }

  async function importSeed() {
    await run(async () => {
      const next = await invoke<VaultStatus>('vault_import_seed', { passphrase, seedHex });
      setVault(next);
      setPassphrase('');
      setSeedHex('');
      setNotice({ kind: 'ok', body: text.generated });
    });
  }

  async function unlockVault() {
    await run(async () => {
      const next = await invoke<VaultStatus>('vault_unlock', { passphrase });
      setVault(next);
      setPassphrase('');
      setNotice({ kind: 'ok', body: text.unlocked });
    });
  }

  async function lockVault() {
    await run(async () => {
      const next = await invoke<VaultStatus>('vault_lock');
      setVault(next);
      setNotice({ kind: 'ok', body: text.locked });
    });
  }

  async function readRequestFile(file: File) {
    const content = await file.text();
    setRequestText(content);
    setRequestName(file.name);
    try {
      const parsed = JSON.parse(content) as { request_id?: string; device_fingerprint?: string };
      if (!customerId && parsed.request_id) {
        setCustomerId(`CUST-${parsed.request_id.slice(0, 8)}`);
      }
    } catch {
      setNotice({ kind: 'bad', body: 'ملف طلب التفعيل ليس بصيغة JSON صالحة.' });
    }
  }

  async function issueSubscription() {
    await run(async () => {
      const outcome = await invoke<IssueResult>('issue_subscription', {
        requestJson: requestText,
        customerId,
        planCode,
        features,
        maxUsers: Number(maxUsers),
        maxBranches: Number(maxBranches),
      });
      setResult(outcome);
      setRequestText('');
      setRequestName('');
      await refreshRows();
      setNotice({
        kind: outcome.warnings.length > 0 ? 'warn' : 'ok',
        body: outcome.warnings.length > 0 ? outcome.warnings.join(' ') : text.issued,
      });
    });
  }

  async function revoke(row: SubscriptionRow) {
    await run(async () => {
      await invoke('revoke_subscription', { licenseId: row.licenseId, reason: 'cancelled by the developer' });
      await refreshRows();
      setNotice({ kind: 'ok', body: text.revoked });
    });
  }

  async function showLicense(row: SubscriptionRow) {
    await run(async () => {
      const content = await invoke<string>('read_license', { licenseId: row.licenseId });
      setResult({
        licenseId: row.licenseId,
        customerId: row.customerId,
        planCode: row.planCode,
        planLabel: row.planCode,
        issuedAtUtc: row.issuedAtUtc,
        expiresAtUtc: row.expiresAtUtc,
        licensePath: row.licenseFile,
        licenseContent: content,
        warnings: [],
      });
      setTab('issue');
    });
  }

  async function copyLicense() {
    if (!result) return;
    try {
      await navigator.clipboard.writeText(result.licenseContent);
      setNotice({ kind: 'ok', body: text.copied });
    } catch {
      setNotice({ kind: 'warn', body: 'تعذّر النسخ التلقائي — انسخ المحتوى يدويًا.' });
    }
  }

  const tabs: Array<{ key: Tab; label: string }> = [
    { key: 'vault', label: text.vault },
    { key: 'subscriptions', label: text.subscriptions },
    { key: 'issue', label: text.issue },
  ];

  return (
    <div className="shell" dir="rtl">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">ن</div>
          <div>
            <strong>{text.title}</strong>
            <small>{text.subtitle}</small>
          </div>
        </div>
        <div className="topbar-right">
          <span className="internal-chip">{text.internal}</span>
          <span className={`vault-chip ${vault?.unlocked ? 'open' : ''}`}>
            <span className="dot" />
            {vault?.unlocked ? text.vaultOpen : text.vaultLocked}
          </span>
        </div>
      </header>

      <main className="body">
        <nav className="tabs">
          {tabs.map((entry) => (
            <button key={entry.key} type="button" className={tab === entry.key ? 'active' : ''} onClick={() => setTab(entry.key)}>
              {entry.label}
            </button>
          ))}
        </nav>

        {notice && <div className={`notice ${notice.kind}`}>{notice.body}</div>}

        {tab === 'vault' && (
          <div className="grid-2">
            <section className="card">
              <h2>{text.vault}</h2>
              <p>{text.vaultWarning}</p>
              <p>{text.vaultDpapi}</p>
              <div className="field">
                <label>{text.publicKey}</label>
                <div className="key-box mono">{vault?.publicKeyHex ?? '—'}</div>
              </div>
              <div className="field">
                <label>{text.registry}</label>
                <div className="key-box mono">{vault?.registryDir ?? '—'}</div>
              </div>
              <div className="row">
                <button className="button" type="button" onClick={() => void lockVault()} disabled={busy || !vault?.unlocked}>{text.lock}</button>
                <button className="button" type="button" onClick={() => void refreshVault()} disabled={busy}>{text.publicKey}</button>
              </div>
            </section>

            <section className="card">
              <h2>{vault?.exists ? text.unlock : text.createVault}</h2>
              <p>{text.passphraseHint}</p>
              <div className="field">
                <label>{text.passphrase}</label>
                <input type="password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} autoComplete="new-password" />
              </div>
              {!vault?.exists && (
                <>
                  <div className="field">
                    <label>{text.confirmPassphrase}</label>
                    <input type="password" value={confirmPassphrase} onChange={(event) => setConfirmPassphrase(event.target.value)} autoComplete="new-password" />
                  </div>
                  <div className="row">
                    <button className="button primary" type="button" onClick={() => void createVault()} disabled={busy || passphrase.length < 12}>{text.createVault}</button>
                  </div>
                  <div className="field">
                    <label>{text.seedHex}</label>
                    <input value={seedHex} onChange={(event) => setSeedHex(event.target.value)} className="mono" />
                  </div>
                  <div className="row">
                    <button className="button" type="button" onClick={() => void importSeed()} disabled={busy || passphrase.length < 12 || seedHex.length !== 64}>{text.importSeed}</button>
                  </div>
                </>
              )}
              {vault?.exists && (
                <div className="row">
                  <button className="button primary" type="button" onClick={() => void unlockVault()} disabled={busy || vault.unlocked}>{text.unlock}</button>
                </div>
              )}
            </section>
          </div>
        )}

        {tab === 'subscriptions' && (
          <section className="card">
            <div className="filter">
              <input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder={text.customerFilter} />
              <span className="mono">{filtered.length}</span>
            </div>
            <div className="scroll">
              {filtered.length === 0 ? (
                <div className="empty">{rows.length === 0 ? text.noSubscriptions : text.noResults}</div>
              ) : (
                <table className="table">
                  <thead>
                    <tr>
                      <th>{text.customer}</th>
                      <th>{text.plan}</th>
                      <th>الحالة</th>
                      <th>ينتهي</th>
                      <th>الجهاز</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {filtered.map((row) => (
                      <tr key={row.licenseId}>
                        <td>{row.customerId}</td>
                        <td>{row.planCode}</td>
                        <td><span className={`pill ${row.revoked ? 'REVOKED' : row.status}`}>{row.revoked ? 'REVOKED' : row.status}</span></td>
                        <td className="mono">{row.expiresAtUtc.slice(0, 10)}</td>
                        <td className="mono">{shortFingerprint(row.deviceFingerprint)}</td>
                        <td>
                          <div className="row">
                            <button className="button" type="button" onClick={() => void showLicense(row)} disabled={busy}>{text.read}</button>
                            {!row.revoked && (
                              <button className="button danger" type="button" onClick={() => void revoke(row)} disabled={busy}>{text.revoke}</button>
                            )}
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
            <div className="notice warn">{text.offlineNote}</div>
          </section>
        )}

        {tab === 'issue' && (
          <div className="grid-2">
            <section className="card">
              <h2>{text.issue}</h2>
              <div className="field">
                <label>{text.requestFile}</label>
                <input type="file" accept=".req,.json,application/json" onChange={(event) => { const file = event.target.files?.[0]; if (file) void readRequestFile(file); }} />
                <small>{text.requestHint} {requestName ? `· ${requestName}` : ''}</small>
              </div>
              <div className="field">
                <label>{text.customer}</label>
                <input value={customerId} onChange={(event) => setCustomerId(event.target.value)} />
              </div>
              <div className="field">
                <label>{text.plan}</label>
                <select value={planCode} onChange={(event) => setPlanCode(event.target.value)}>
                  {(catalog?.plans ?? []).map((plan) => (
                    <option key={plan.code} value={plan.code}>{plan.label}</option>
                  ))}
                </select>
              </div>
              <div className="field">
                <label>{text.features}</label>
                <div className="check-list">
                  {(catalog?.features ?? []).map((feature) => (
                    <label className="check" key={feature}>
                      <input
                        type="checkbox"
                        checked={features.includes(feature)}
                        onChange={(event) => setFeatures((current) => event.target.checked ? [...current, feature] : current.filter((item) => item !== feature))}
                      />
                      <span>{feature}</span>
                    </label>
                  ))}
                </div>
              </div>
              <div className="row">
                <div className="field">
                  <label>{text.maxUsers}</label>
                  <input type="number" min="1" value={maxUsers} onChange={(event) => setMaxUsers(event.target.value)} />
                </div>
                <div className="field">
                  <label>{text.maxBranches}</label>
                  <input type="number" min="1" value={maxBranches} onChange={(event) => setMaxBranches(event.target.value)} />
                </div>
              </div>
              <div className="row">
                <button className="button primary" type="button" onClick={() => void issueSubscription()} disabled={busy || !requestText || !customerId || !vault?.unlocked}>
                  {text.issueButton}
                </button>
              </div>
            </section>

            <section className="card">
              <h2>{text.licenseFile}</h2>
              {result ? (
                <>
                  <div className="key-box mono">{result.licensePath}</div>
                  <div className="result">{result.licenseContent}</div>
                  <div className="row">
                    <button className="button" type="button" onClick={() => void copyLicense()}>{text.copy}</button>
                  </div>
                </>
              ) : (
                <p>لم تُصدر رخصة بعد من هذه الجلسة.</p>
              )}
            </section>
          </div>
        )}
      </main>
    </div>
  );
}
