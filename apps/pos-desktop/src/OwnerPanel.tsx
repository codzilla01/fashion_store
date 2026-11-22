import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Lang } from './lang';
import {
  currentRpId,
  getPlatformAssertion,
  platformAuthenticatorAvailable,
  registerPlatformAuthenticator,
  webauthnSupported,
} from './webauthn';

/** The owner that is behind the panel. */
type OwnerIdentity = { userId: string; displayName: string; username: string };

/** What the panel knows about itself before it is unlocked. */
type OwnerPanelStatus = {
  ownerConfigured: boolean;
  owners: OwnerIdentity[];
  credentialsEnrolled: number;
  thisDeviceEnrolled: boolean;
  sessionActive: boolean;
  lockedUntilUtc: string | null;
  rpId: string;
};

/** The result of an unlock. */
type OwnerAuthResult = {
  ownerToken: string;
  displayName: string;
  authMethod: string;
  expiresInSeconds: number;
};

/** A pending ceremony. */
type OwnerChallenge = {
  challengeId: string;
  challenge: string;
  rpId: string;
  userId: string;
  credentialIds: string[];
  expiresInSeconds: number;
};

/** An enrolled authenticator. */
type OwnerCredential = {
  id: string;
  label: string;
  algorithm: string;
  rpId: string;
  enrolledAt: string;
  lastUsedAt: string | null;
  revokedAt: string | null;
  credentialId: string;
};

/** The return policy the owner controls. */
type ReturnPolicy = {
  periodDays: number;
  allowDiscountedItems: boolean;
  restockConditions: string[];
};

/** One branch of the store. */
type BranchSummary = {
  id: string;
  code: string;
  nameAr: string;
  nameEn: string;
  taxRateBps: number;
  pricesIncludeTax: boolean;
};

/** How many accounts carry a role. */
type RoleCount = { roleCode: string; nameAr: string; accounts: number };

/** The store settings the owner controls. */
type OwnerOverview = {
  branchId: string;
  branchCode: string;
  branchNameAr: string;
  taxRateBps: number;
  pricesIncludeTax: boolean;
  returnPolicy: ReturnPolicy | null;
  branches: BranchSummary[];
  usersByRole: RoleCount[];
  licenseValid: boolean;
  licenseStatusCode: string;
  licenseExpiresAtUtc: string | null;
  planCode: string | null;
  customers: number;
  products: number;
  variants: number;
  invoicesTotal: number;
  ownerName: string;
  rpId: string;
};

type OwnerTab = 'overview' | 'returns' | 'tax' | 'security';

const messages = {
  ar: {
    title: 'لوحة تحكم المالك',
    subtitle: 'إعدادات المتجر وسياسة المرتجعات والضريبة وأمان الدخول',
    close: 'إغلاق',
    unlock: 'فتح اللوحة',
    unlockHint: 'اضغط مطوّلًا على شعار التطبيق في أي وقت لفتح هذه اللوحة.',
    biometric: 'فتح ببصمة الجهاز',
    biometricShort: 'بصمة الجهاز',
    passwordShort: 'كلمة المرور',
    biometricHint: 'Windows Hello على الكمبيوتر، أو بصمة الجهاز على الجوال.',
    biometricUnavailable: 'بصمة الجهاز غير متاحة على هذا الجهاز — استخدم كلمة المرور.',
    passwordFallback: 'أو الدخول بكلمة مرور المالك',
    username: 'اسم المستخدم',
    password: 'كلمة المرور',
    signIn: 'دخول',
    working: 'جارٍ التحقق…',
    overview: 'نظرة عامة',
    returns: 'سياسة المرتجعات',
    tax: 'الضريبة',
    security: 'الأجهزة والأمان',
    lock: 'قفل اللوحة',
    periodDays: 'مدة الاسترجاع (بالأيام)',
    periodHint: 'بعد هذه المدة يُرفض المرتجع من مكتب المبيعات.',
    allowDiscounted: 'السماح بإرجاع الأصناف المخفّضة',
    allowDiscountedHint: 'عند الإيقاف يُرفض إرجاع أي فاتورة عليها خصم.',
    restockConditions: 'الحالات المسموح بإرجاعها للمخزون',
    restockHint: 'ما عدا ذلك يذهب المرتجع إلى التالف أو الحجز.',
    save: 'حفظ',
    saved: 'تم الحفظ',
    taxRate: 'نسبة الضريبة (%)',
    taxMode: 'طريقة عرض السعر',
    inclusive: 'السعر شامل الضريبة',
    exclusive: 'السعر غير شامل الضريبة',
    taxHint: 'الضريبة 14% — تُضاف على السعر المعروض أو تكون داخله حسب الاختيار.',
    branch: 'الفرع',
    license: 'الترخيص',
    valid: 'ساري',
    invalid: 'غير ساري',
    expires: 'ينتهي في',
    customers: 'العملاء',
    products: 'الأصناف',
    invoices: 'الفواتير',
    accounts: 'الحسابات',
    enroll: 'تسجيل بصمة هذا الجهاز',
    enrolled: 'الأجهزة المسجّلة',
    noCredentials: 'لا توجد أجهزة مسجّلة بعد — سجّل بصمة هذا الجهاز لتدخل لاحقًا بدون كلمة مرور.',
    revoke: 'إلغاء التسجيل',
    revoked: 'ملغى',
    lastUsed: 'آخر استخدام',
    never: 'لم يُستخدم بعد',
    biometricReady: 'بصمة الجهاز مفعّلة على هذا الجهاز',
    biometricMissing: 'لم تُسجّل بصمة هذا الجهاز بعد',
    panelLocked: 'اللوحة مقفلة مؤقتًا بعد محاولات فاشلة. حاول بعد قليل.',
    sessionExpired: 'انتهت جلسة اللوحة — أعد الفتح.',
    desktopOnly: 'لوحة المالك متاحة في تطبيق سطح المكتب فقط.',
  },
  en: {
    title: 'Owner control panel',
    subtitle: 'Store settings, return policy, tax and access security',
    close: 'Close',
    unlock: 'Unlock the panel',
    unlockHint: 'Long press the application logo at any time to open this panel.',
    biometric: 'Unlock with the device fingerprint',
    biometricShort: 'Device fingerprint',
    passwordShort: 'Password',
    biometricHint: 'Windows Hello on a computer, or the device fingerprint on a phone.',
    biometricUnavailable: 'No device fingerprint on this machine — use the password instead.',
    passwordFallback: 'Or sign in with the owner password',
    username: 'Username',
    password: 'Password',
    signIn: 'Sign in',
    working: 'Verifying…',
    overview: 'Overview',
    returns: 'Return policy',
    tax: 'Tax',
    security: 'Devices and security',
    lock: 'Lock the panel',
    periodDays: 'Return window (days)',
    periodHint: 'After this window the returns desk refuses the return.',
    allowDiscounted: 'Allow returns of discounted items',
    allowDiscountedHint: 'When off, every invoice that carries a discount is refused.',
    restockConditions: 'Conditions that may go back into stock',
    restockHint: 'Anything else goes to damaged or quarantine.',
    save: 'Save',
    saved: 'Saved',
    taxRate: 'Tax rate (%)',
    taxMode: 'Price display',
    inclusive: 'Prices include tax',
    exclusive: 'Prices exclude tax',
    taxHint: 'The tax is 14% — it is added on top of the price or included in it, as chosen.',
    branch: 'Branch',
    license: 'Licence',
    valid: 'valid',
    invalid: 'not valid',
    expires: 'expires on',
    customers: 'Customers',
    products: 'Products',
    invoices: 'Invoices',
    accounts: 'Accounts',
    enroll: 'Enrol this device fingerprint',
    enrolled: 'Enrolled devices',
    noCredentials: 'No device is enrolled yet — enrol the fingerprint of this device to sign in without a password.',
    revoke: 'Revoke',
    revoked: 'revoked',
    lastUsed: 'Last used',
    never: 'never used',
    biometricReady: 'The device fingerprint is enrolled on this machine',
    biometricMissing: 'The fingerprint of this device is not enrolled yet',
    panelLocked: 'The panel is locked after failed attempts. Try again shortly.',
    sessionExpired: 'The panel session expired — unlock again.',
    desktopOnly: 'The owner panel is available in the desktop application only.',
  },
} as const;

const ERROR_TEXT: Record<string, { ar: string; en: string }> = {
  OWNER_LOGIN_FAILED: { ar: 'بيانات الدخول غير صحيحة.', en: 'Those credentials are not correct.' },
  OWNER_PANEL_LOCKED: { ar: 'اللوحة مقفلة مؤقتًا بعد محاولات فاشلة.', en: 'The panel is locked after failed attempts.' },
  OWNER_SESSION_REQUIRED: { ar: 'انتهت جلسة اللوحة — أعد الفتح.', en: 'The panel session expired — unlock again.' },
  OWNER_NO_AUTHENTICATOR_ENROLLED: { ar: 'لا توجد بصمة مسجّلة — سجّل بصمة هذا الجهاز من تبويب الأمان.', en: 'No fingerprint is enrolled — enrol this device in the security tab.' },
  OWNER_ACCOUNT_REQUIRED: { ar: 'يوجد أكثر من حساب مالك — اختر الحساب.', en: 'There is more than one owner account — choose one.' },
  OWNER_RELYING_PARTY_MISMATCH: { ar: 'تعذّر التحقق من أصل الطلب. أعد المحاولة من التطبيق نفسه.', en: 'The origin of the request could not be verified. Try again from the application.' },
  OWNER_RELYING_PARTY_NOT_ALLOWED: { ar: 'هذا الأصل غير مسموح له بفتح لوحة المالك.', en: 'This origin is not allowed to open the owner panel.' },
  OWNER_CREDENTIAL_UNKNOWN: { ar: 'لم يتم العثور على بصمة مسجّلة لهذا الجهاز.', en: 'No enrolled fingerprint was found for this device.' },
  OWNER_CREDENTIAL_REVOKED: { ar: 'تم إلغاء تسجيل هذه البصمة.', en: 'This fingerprint was revoked.' },
  OWNER_BIOMETRIC_NOT_VERIFIED: { ar: 'لم يتم التحقق من بصمة الجهاز. فعّل Windows Hello أو البصمة أولًا.', en: 'The device fingerprint was not verified. Turn on Windows Hello or the fingerprint first.' },
  OWNER_PRESENCE_NOT_VERIFIED: { ar: 'لم يتم التأكد من حضور المستخدم.', en: 'The presence of the user was not confirmed.' },
  OWNER_SIGNATURE_INVALID: { ar: 'توقيع البصمة غير صالح.', en: 'The fingerprint signature is not valid.' },
  OWNER_CLIENT_DATA_INVALID: { ar: 'بيانات التحقق غير صالحة.', en: 'The verification data is not valid.' },
  OWNER_CHALLENGE_EXPIRED: { ar: 'انتهت صلاحية الطلب. أعد المحاولة.', en: 'The request expired. Try again.' },
  OWNER_CHALLENGE_INVALID: { ar: 'طلب التحقق غير صالح.', en: 'The verification request is not valid.' },
  OWNER_ATTESTATION_INCOMPLETE: { ar: 'بيانات تسجيل البصمة غير مكتملة.', en: 'The enrolment data is incomplete.' },
  WEBAUTHN_NOT_SUPPORTED: { ar: 'هذا المتصفح لا يدعم تسجيل البصمة. استخدم كلمة المرور.', en: 'This browser cannot enrol a fingerprint. Use the password instead.' },
  WEBAUTHN_CANCELLED: { ar: 'تم إلغاء عملية التحقق.', en: 'The verification was cancelled.' },
  RETURN_POLICY_INVALID: { ar: 'قيمة سياسة المرتجعات غير مقبولة.', en: 'The return policy values are not acceptable.' },
  TAX_RATE_INVALID: { ar: 'نسبة الضريبة غير مقبولة.', en: 'The tax rate is not acceptable.' },
  OWNER_BRANCH_NOT_FOUND: { ar: 'الفرع غير موجود.', en: 'That branch does not exist.' },
  OWNER_NO_BRANCH: { ar: 'لا يوجد فرع في المتجر بعد.', en: 'The store has no branch yet.' },
};

function describeError(raw: unknown, lang: Lang): string {
  const code = String(raw);
  const known = ERROR_TEXT[code];
  if (known) return known[lang];
  const clean = code.replace(/^Error:\s*/, '').split('\n')[0];
  return clean || (lang === 'ar' ? 'حدث خطأ غير متوقع.' : 'Something went wrong.');
}

function formatDate(value: string | null, lang: Lang): string {
  if (!value) return '—';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  return parsed.toLocaleString(lang === 'ar' ? 'ar-EG' : 'en-GB', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

export default function OwnerPanel({ lang, onClose }: { lang: Lang; onClose: () => void }) {
  const t = messages[lang];
  const dir = lang === 'ar' ? 'rtl' : 'ltr';
  const [stage, setStage] = useState<'gate' | 'panel'>('gate');
  const [status, setStatus] = useState<OwnerPanelStatus | null>(null);
  const [ownerToken, setOwnerToken] = useState('');
  const [authMethod, setAuthMethod] = useState('');
  const [credentials, setCredentials] = useState<OwnerCredential[]>([]);
  const [overview, setOverview] = useState<OwnerOverview | null>(null);
  const [tab, setTab] = useState<OwnerTab>('overview');
  const [branchId, setBranchId] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [form, setForm] = useState({ username: '', password: '' });
  const [policyForm, setPolicyForm] = useState({
    periodDays: '14',
    allowDiscountedItems: true,
    restockConditions: ['UNOPENED', 'NEW_TAGS'] as string[],
  });
  const [taxForm, setTaxForm] = useState({ rate: '14', inclusive: false });
  const [platformReady, setPlatformReady] = useState(false);

  const isDesktop = Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

  const loadStatus = useCallback(async () => {
    if (!isDesktop) return;
    try {
      const next = await invoke<OwnerPanelStatus>('owner_panel_status');
      setStatus(next);
    } catch (failure) {
      setError(describeError(failure, lang));
    }
  }, [isDesktop, lang]);

  useEffect(() => {
    void loadStatus();
    void platformAuthenticatorAvailable().then(setPlatformReady);
  }, [loadStatus]);

  const loadOverview = useCallback(async (token: string, branch: string) => {
    const next = await invoke<OwnerOverview>('owner_panel_overview', {
      ownerToken: token,
      branchId: branch || null,
    });
    setOverview(next);
    setBranchId(next.branchId);
    setPolicyForm({
      periodDays: String(next.returnPolicy?.periodDays ?? 14),
      allowDiscountedItems: next.returnPolicy?.allowDiscountedItems ?? true,
      restockConditions: next.returnPolicy?.restockConditions ?? ['UNOPENED', 'NEW_TAGS'],
    });
    setTaxForm({
      rate: String(next.taxRateBps / 100),
      inclusive: next.pricesIncludeTax,
    });
  }, []);

  const loadCredentials = useCallback(async (token: string) => {
    const next = await invoke<OwnerCredential[]>('owner_panel_credentials', { ownerToken: token });
    setCredentials(next);
  }, []);

  const unlock = useCallback(async (token: string, method: string) => {
    setOwnerToken(token);
    setAuthMethod(method);
    setStage('panel');
    setError('');
    try {
      await loadOverview(token, '');
      await loadCredentials(token);
    } catch (failure) {
      setError(describeError(failure, lang));
    }
  }, [lang, loadCredentials, loadOverview]);

  async function unlockWithPassword() {
    setBusy(true);
    setError('');
    try {
      const result = await invoke<OwnerAuthResult>('owner_panel_unlock_with_password', {
        username: form.username,
        password: form.password,
      });
      setForm((current) => ({ ...current, password: '' }));
      await unlock(result.ownerToken, result.authMethod);
    } catch (failure) {
      setError(describeError(failure, lang));
      await loadStatus();
    } finally {
      setBusy(false);
    }
  }

  async function unlockWithBiometric() {
    setBusy(true);
    setError('');
    try {
      const ownerUserId = status?.owners.length === 1 ? status.owners[0].userId : null;
      const challenge = await invoke<OwnerChallenge>('owner_panel_begin_assertion', {
        ownerUserId,
        rpId: currentRpId(),
      });
      const assertion = await getPlatformAssertion({
        challenge: challenge.challenge,
        rpId: challenge.rpId,
        credentialIds: challenge.credentialIds,
      });
      const result = await invoke<OwnerAuthResult>('owner_panel_finish_assertion', {
        input: {
          challengeId: challenge.challengeId,
          credentialId: assertion.credentialId,
          authenticatorData: assertion.authenticatorData,
          clientDataJson: assertion.clientDataJson,
          signature: assertion.signature,
        },
      });
      await unlock(result.ownerToken, result.authMethod);
    } catch (failure) {
      setError(describeError(failure, lang));
      await loadStatus();
    } finally {
      setBusy(false);
    }
  }

  async function enrolDevice() {
    setBusy(true);
    setError('');
    try {
      const challenge = await invoke<OwnerChallenge>('owner_panel_begin_registration', {
        ownerToken,
        rpId: currentRpId(),
      });
      const registration = await registerPlatformAuthenticator({
        challenge: challenge.challenge,
        rpId: challenge.rpId,
        userId: challenge.userId,
        userName: overview?.ownerName ?? 'owner',
      });
      await invoke('owner_panel_finish_registration', {
        input: {
          challengeId: challenge.challengeId,
          ownerToken,
          credentialId: registration.credentialId,
          attestationObject: registration.attestationObject,
          clientDataJson: registration.clientDataJson,
          label: platformReady ? 'Windows Hello' : 'Device authenticator',
        },
      });
      await loadCredentials(ownerToken);
      setNotice(lang === 'ar' ? 'تم تسجيل بصمة هذا الجهاز.' : 'The fingerprint of this device is enrolled.');
      await loadStatus();
    } catch (failure) {
      setError(describeError(failure, lang));
    } finally {
      setBusy(false);
    }
  }

  async function revokeCredential(id: string) {
    setBusy(true);
    setError('');
    try {
      await invoke('owner_panel_revoke_credential', { ownerToken, credentialId: id });
      await loadCredentials(ownerToken);
      setNotice(lang === 'ar' ? 'تم إلغاء تسجيل البصمة.' : 'The fingerprint was revoked.');
    } catch (failure) {
      setError(describeError(failure, lang));
    } finally {
      setBusy(false);
    }
  }

  async function savePolicy() {
    setBusy(true);
    setError('');
    try {
      const days = Number(policyForm.periodDays);
      if (!Number.isFinite(days) || days < 1 || days > 180) {
        throw new Error('RETURN_POLICY_INVALID');
      }
      await invoke('owner_panel_set_return_policy', {
        ownerToken,
        branchId,
        policy: {
          periodDays: Math.trunc(days),
          allowDiscountedItems: policyForm.allowDiscountedItems,
          restockConditions: policyForm.restockConditions,
        },
      });
      setNotice(t.saved);
      await loadOverview(ownerToken, branchId);
    } catch (failure) {
      setError(describeError(failure, lang));
    } finally {
      setBusy(false);
    }
  }

  async function saveTax() {
    setBusy(true);
    setError('');
    try {
      const rate = Number(taxForm.rate);
      if (!Number.isFinite(rate) || rate < 0 || rate > 100) {
        throw new Error('TAX_RATE_INVALID');
      }
      await invoke('owner_panel_set_tax', {
        ownerToken,
        branchId,
        taxRateBps: Math.round(rate * 100),
        pricesIncludeTax: taxForm.inclusive,
      });
      setNotice(t.saved);
      await loadOverview(ownerToken, branchId);
    } catch (failure) {
      setError(describeError(failure, lang));
    } finally {
      setBusy(false);
    }
  }

  async function lockPanel() {
    try {
      await invoke('owner_panel_lock', { ownerToken });
    } catch {
      // The panel is closed either way.
    }
    setOwnerToken('');
    setAuthMethod('');
    setOverview(null);
    setCredentials([]);
    setStage('gate');
    await loadStatus();
  }

  const tabs: Array<{ key: OwnerTab; label: string }> = [
    { key: 'overview', label: t.overview },
    { key: 'returns', label: t.returns },
    { key: 'tax', label: t.tax },
    { key: 'security', label: t.security },
  ];

  return (
    <div className="owner-overlay" dir={dir}>
      <div className="owner-shell">
        <header className="owner-head">
          <div className="owner-head-brand">
            <div className="owner-mark">ن</div>
            <div>
              <strong>{t.title}</strong>
              <small>{t.subtitle}</small>
            </div>
          </div>
          <div className="owner-head-actions">
            {stage === 'panel' && (
              <>
                <span className="owner-auth-method">
                  {authMethod === 'BIOMETRIC' ? t.biometricShort : t.passwordShort}
                </span>
                <button className="owner-ghost-button" type="button" onClick={() => void lockPanel()}>{t.lock}</button>
              </>
            )}
            <button className="owner-ghost-button" type="button" onClick={onClose}>{t.close}</button>
          </div>
        </header>

        {stage === 'gate' ? (
          <div className="owner-gate">
            <div className="owner-card-mark">
              <span className="owner-card-glyph">⌾</span>
            </div>
            <h1>{t.unlock}</h1>
            <p>{t.unlockHint}</p>
            {!isDesktop && <div className="auth-warning">{t.desktopOnly}</div>}
            {status?.lockedUntilUtc && <div className="auth-warning">{t.panelLocked}</div>}
            <button
              className="owner-primary"
              type="button"
              onClick={() => void unlockWithBiometric()}
              disabled={busy || !isDesktop || !webauthnSupported() || (status?.credentialsEnrolled ?? 0) === 0}
            >
              {busy ? t.working : t.biometric}
            </button>
            <small className="owner-hint">
              {webauthnSupported() ? t.biometricHint : t.biometricUnavailable}
            </small>
            <div className="owner-divider"><span>{t.passwordFallback}</span></div>
            <form className="auth-form" onSubmit={(event) => { event.preventDefault(); void unlockWithPassword(); }}>
              <label>
                {t.username}
                <input
                  value={form.username}
                  onChange={(event) => setForm((current) => ({ ...current, username: event.target.value }))}
                  autoComplete="username"
                  required
                />
              </label>
              <label>
                {t.password}
                <input
                  type="password"
                  value={form.password}
                  onChange={(event) => setForm((current) => ({ ...current, password: event.target.value }))}
                  autoComplete="current-password"
                  required
                />
              </label>
              {error && <div className="auth-error">{error}</div>}
              <button className="auth-submit" type="submit" disabled={busy || !isDesktop}>{busy ? t.working : t.signIn}</button>
            </form>
          </div>
        ) : (
          <div className="owner-body">
            <nav className="owner-tabs">
              {tabs.map((entry) => (
                <button
                  key={entry.key}
                  type="button"
                  className={tab === entry.key ? 'active' : ''}
                  onClick={() => setTab(entry.key)}
                >
                  {entry.label}
                </button>
              ))}
            </nav>

            {error && <div className="auth-error owner-error">{error}</div>}
            {notice && <div className="auth-helper owner-notice">{notice}</div>}

            {tab === 'overview' && overview && (
              <div className="owner-grid">
                <section className="owner-card">
                  <span className="owner-card-label">{t.license}</span>
                  <strong className={overview.licenseValid ? 'ok' : 'bad'}>
                    {overview.licenseValid ? t.valid : t.invalid} · {overview.licenseStatusCode}
                  </strong>
                  {overview.licenseExpiresAtUtc && (
                    <small>{t.expires} {formatDate(overview.licenseExpiresAtUtc, lang)}</small>
                  )}
                  {overview.planCode && <small>{overview.planCode}</small>}
                </section>
                <section className="owner-card">
                  <span className="owner-card-label">{t.customers}</span>
                  <strong>{overview.customers}</strong>
                  <small>{overview.products} {lang === 'ar' ? 'صنف' : 'products'} · {overview.variants} {lang === 'ar' ? 'متغير' : 'variants'}</small>
                </section>
                <section className="owner-card">
                  <span className="owner-card-label">{t.invoices}</span>
                  <strong>{overview.invoicesTotal}</strong>
                  <small>{lang === 'ar' ? 'إجمالي الفواتير المحفوظة' : 'invoices stored'}</small>
                </section>
                <section className="owner-card">
                  <span className="owner-card-label">{t.accounts}</span>
                  <strong>{overview.usersByRole.reduce((sum, role) => sum + role.accounts, 0)}</strong>
                  <small>{overview.usersByRole.map((role) => `${role.nameAr}: ${role.accounts}`).join(' · ')}</small>
                </section>
              </div>
            )}

            {tab === 'returns' && (
              <section className="owner-panel-section">
                <div className="owner-field">
                  <label htmlFor="owner-period">{t.periodDays}</label>
                  <input
                    id="owner-period"
                    type="number"
                    min={1}
                    max={180}
                    value={policyForm.periodDays}
                    onChange={(event) => setPolicyForm((current) => ({ ...current, periodDays: event.target.value }))}
                  />
                  <small>{t.periodHint}</small>
                </div>
                <div className="owner-field">
                  <label className="owner-check">
                    <input
                      type="checkbox"
                      checked={policyForm.allowDiscountedItems}
                      onChange={(event) => setPolicyForm((current) => ({ ...current, allowDiscountedItems: event.target.checked }))}
                    />
                    <span>{t.allowDiscounted}</span>
                  </label>
                  <small>{t.allowDiscountedHint}</small>
                </div>
                <div className="owner-field">
                  <label>{t.restockConditions}</label>
                  <div className="owner-check-list">
                    {(['UNOPENED', 'NEW_TAGS'] as const).map((condition) => (
                      <label className="owner-check" key={condition}>
                        <input
                          type="checkbox"
                          checked={policyForm.restockConditions.includes(condition)}
                          onChange={(event) => setPolicyForm((current) => ({
                            ...current,
                            restockConditions: event.target.checked
                              ? [...current.restockConditions, condition]
                              : current.restockConditions.filter((item) => item !== condition),
                          }))}
                        />
                        <span>{condition === 'UNOPENED'
                          ? (lang === 'ar' ? 'غير مفتوحة' : 'Unopened')
                          : (lang === 'ar' ? 'بالبطاقة' : 'With tags')}</span>
                      </label>
                    ))}
                  </div>
                  <small>{t.restockHint}</small>
                </div>
                <div className="owner-actions">
                  <button className="owner-primary" type="button" onClick={() => void savePolicy()} disabled={busy}>{t.save}</button>
                </div>
              </section>
            )}

            {tab === 'tax' && (
              <section className="owner-panel-section">
                <div className="owner-field">
                  <label>{t.taxMode}</label>
                  <div className="tax-choice">
                    <button
                      type="button"
                      className={taxForm.rate !== '0' ? 'chosen' : ''}
                      onClick={() => setTaxForm((current) => ({ ...current, rate: current.rate === '0' ? '14' : current.rate }))}
                    >
                      {lang === 'ar' ? 'الضريبة مفعّلة' : 'Tax on'}
                    </button>
                    <button
                      type="button"
                      className={taxForm.rate === '0' ? 'chosen' : ''}
                      onClick={() => setTaxForm((current) => ({ ...current, rate: '0' }))}
                    >
                      {lang === 'ar' ? 'بدون ضريبة' : 'No tax'}
                    </button>
                  </div>
                  <small>{lang === 'ar' ? 'الضريبة اختيارية. عند اختيار «بدون ضريبة» تُحفظ النسبة صفرًا ولا تُضاف أي ضريبة على الفواتير.' : 'Tax is optional. Choosing "No tax" stores a zero rate and no tax is added to invoices.'}</small>
                </div>
                <div className="owner-field">
                  <label htmlFor="owner-tax-rate">{t.taxRate}</label>
                  <input
                    id="owner-tax-rate"
                    type="number"
                    min={0}
                    max={100}
                    step="0.01"
                    value={taxForm.rate}
                    onChange={(event) => setTaxForm((current) => ({ ...current, rate: event.target.value }))}
                  />
                  <small>{t.taxHint}</small>
                </div>
                <div className="owner-field">
                  <label>{t.taxMode}</label>
                  <div className="tax-choice">
                    <button
                      type="button"
                      className={!taxForm.inclusive ? 'chosen' : ''}
                      onClick={() => setTaxForm((current) => ({ ...current, inclusive: false }))}
                    >
                      {t.exclusive}
                    </button>
                    <button
                      type="button"
                      className={taxForm.inclusive ? 'chosen' : ''}
                      onClick={() => setTaxForm((current) => ({ ...current, inclusive: true }))}
                    >
                      {t.inclusive}
                    </button>
                  </div>
                </div>
                <div className="owner-actions">
                  <button className="owner-primary" type="button" onClick={() => void saveTax()} disabled={busy}>{t.save}</button>
                </div>
              </section>
            )}

            {tab === 'security' && (
              <section className="owner-panel-section">
                <div className="owner-field">
                  <label>{t.enrolled}</label>
                  {credentials.length === 0 && <small>{t.noCredentials}</small>}
                  <div className="owner-credential-list">
                    {credentials.map((credential) => (
                      <div className="owner-credential" key={credential.id}>
                        <div>
                          <strong>{credential.label}</strong>
                          <small>
                            {credential.algorithm} · {t.lastUsed}: {credential.lastUsedAt ? formatDate(credential.lastUsedAt, lang) : t.never}
                          </small>
                          {credential.revokedAt && <small className="bad">{t.revoked}</small>}
                        </div>
                        {!credential.revokedAt && (
                          <button className="owner-ghost-button" type="button" onClick={() => void revokeCredential(credential.id)} disabled={busy}>
                            {t.revoke}
                          </button>
                        )}
                      </div>
                    ))}
                  </div>
                  <small>{platformReady ? t.biometricReady : t.biometricMissing}</small>
                </div>
                <div className="owner-actions">
                  <button className="owner-primary" type="button" onClick={() => void enrolDevice()} disabled={busy || !webauthnSupported()}>
                    {t.enroll}
                  </button>
                </div>
              </section>
            )}

            {overview && overview.branches.length > 1 && (
              <div className="owner-branch-bar">
                <label htmlFor="owner-branch">{t.branch}</label>
                <select
                  id="owner-branch"
                  value={branchId}
                  onChange={(event) => {
                    setBranchId(event.target.value);
                    void loadOverview(ownerToken, event.target.value);
                  }}
                >
                  {overview.branches.map((branch) => (
                    <option key={branch.id} value={branch.id}>
                      {branch.code} · {lang === 'ar' ? branch.nameAr : branch.nameEn}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
