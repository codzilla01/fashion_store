import { useMemo, useState, type FormEvent } from 'react';

export type ManagerSection = 'overview' | 'orders' | 'support' | 'activity';
type PosTarget = 'sale' | 'products' | 'users' | 'reports' | 'branches' | 'backup';

type Props = {
  section: ManagerSection;
  onSectionChange: (section: ManagerSection) => void;
  onNavigate: (tab: PosTarget) => void;
};

type ThreadMessage = {
  id: string;
  author: 'customer' | 'agent';
  text: string;
  time: string;
};

type Conversation = {
  id: string;
  name: string;
  subject: string;
  preview: string;
  ticket: string;
  date: string;
  status: 'جديد' | 'قيد المعالجة' | 'بانتظار العميل' | 'تم الحل';
  statusTone: 'new' | 'progress' | 'waiting' | 'done';
  messages: ThreadMessage[];
};

const initialConversations: Conversation[] = [
  {
    id: 'tk-0248',
    name: 'أحمد محمد',
    subject: 'استفسار عن ميعاد توصيل الطلب',
    preview: 'استفسار عن ميعاد توصيل الطلب TK-0248',
    ticket: 'TK-0248',
    date: 'الأحد، ١٧ نوفمبر',
    status: 'جديد',
    statusTone: 'new',
    messages: [
      { id: 'm1', author: 'customer', text: 'استفسار عن ميعاد توصيل الطلب', time: '١٧ نوفمبر ٢٠٢٦، ١١:٤٨ م' },
      { id: 'm2', author: 'agent', text: 'أهلًا بك عميلنا الكريم، يسعدنا إفادتك بأن طلبك في طريقة التوصيل وسيصل خلال يوم عمل واحد. شكرًا لصبرك.', time: '١٧ نوفمبر ٢٠٢٦، ١٢:٠٢ م' },
    ],
  },
  {
    id: 'tk-0247',
    name: 'سارة علي',
    subject: 'تعديل عنوان الشحن',
    preview: 'هل يمكن تعديل عنوان الشحن من قبل الطالب؟',
    ticket: 'TK-0247',
    date: 'الجمعة، ١٥ نوفمبر',
    status: 'قيد المعالجة',
    statusTone: 'progress',
    messages: [
      { id: 'm1', author: 'customer', text: 'هل يمكن تعديل عنوان الشحن من قبل الطالب؟', time: '١٥ نوفمبر ٢٠٢٦، ٩:٢٤ م' },
      { id: 'm2', author: 'agent', text: 'أهلًا سارة، لقد أرسلنا لك رابطًا لتأكيد العنوان الصحيح وسنعتبره التحديث النهائي للشحنة.', time: '١٥ نوفمبر ٢٠٢٦، ١٠:١١ م' },
      { id: 'm3', author: 'customer', text: 'تم تأكيد العنوان الجديد، شكرًا لتعاونكم.', time: '١٦ نوفمبر ٢٠٢٦، ٨:٣٠ ص' },
    ],
  },
  {
    id: 'tk-0246',
    name: 'محمد عادل',
    subject: 'مساعدة في اختيار المقاسات',
    preview: 'مساعدة في اختيار المقاسات المناسبة',
    ticket: 'TK-0246',
    date: 'الخميس، ١٤ نوفمبر',
    status: 'بانتظار العميل',
    statusTone: 'waiting',
    messages: [
      { id: 'm1', author: 'customer', text: 'مساعدة في اختيار المقاسات المناسبة لطولي ووزني.', time: '١٤ نوفمبر ٢٠٢٦، ١:١٥ م' },
      { id: 'm2', author: 'agent', text: 'يسعدنا خدمتك! أرسل لنا مقاس الصدر والخصر وسنرشح لك المقاس الأنسب من جدول المقاسات.', time: '١٤ نوفمبر ٢٠٢٦، ١:٤٠ م' },
    ],
  },
  {
    id: 'tk-0245',
    name: 'مريم حسن',
    subject: 'شكوى من سرعة التوصيل',
    preview: 'شكوى من سرعة التوصيل وجودة التغليف',
    ticket: 'TK-0245',
    date: 'الأربعاء، ١٣ نوفمبر',
    status: 'تم الحل',
    statusTone: 'done',
    messages: [
      { id: 'm1', author: 'customer', text: 'شكوى من سرعة التوصيل وجودة التغليف.', time: '١٣ نوفمبر ٢٠٢٦، ٥:٣٠ م' },
      { id: 'm2', author: 'agent', text: 'نعتذر منك، تمت مراجعة الشحنة مع شركة التوصيل وسيتم تحسين التغليف في الطلبات القادمة.', time: '١٣ نوفمبر ٢٠٢٦، ٦:١٢ م' },
    ],
  },
];

const orders = [
  { id: '#OR-1052', customer: 'أحمد محمد', date: '٢١ نوفمبر ٢٠٢٦', items: 3, total: '١٬٤٨٠ ج.م', status: 'قيد المراجعة', tone: 'progress' },
  { id: '#OR-1051', customer: 'سارة علي', date: '٢٠ نوفمبر ٢٠٢٦', items: 2, total: '٩٦٥ ج.م', status: 'قيد التجهيز', tone: 'waiting' },
  { id: '#OR-1050', customer: 'محمد عادل', date: '٢٠ نوفمبر ٢٠٢٦', items: 4, total: '٢٬٣٥٠ ج.م', status: 'قيد الشحن', tone: 'shipping' },
  { id: '#OR-1049', customer: 'مريم حسن', date: '١٩ نوفمبر ٢٠٢٦', items: 1, total: '٧٤٥ ج.م', status: 'تم التوصيل', tone: 'done' },
  { id: '#OR-1048', customer: 'فاطمة أحمد', date: '١٩ نوفمبر ٢٠٢٦', items: 2, total: '١٬١٩٠ ج.م', status: 'تم التوصيل', tone: 'done' },
];

const activity = [
  { title: 'تم تحديث حالة الطلب #OR-1052', detail: 'بانتظار مراجعة الإدارة', time: 'قبل ١٢ دقيقة', tone: 'rose' },
  { title: 'رسالة جديدة من أحمد محمد', detail: 'استفسار عن ميعاد توصيل الطلب TK-0248', time: 'قبل ٤٥ دقيقة', tone: 'blue' },
  { title: 'تمت إضافة صنف جديد', detail: 'بلوزة ساتان ناعمة — مقاسات S إلى XL', time: 'قبل ساعتين', tone: 'green' },
  { title: 'تم حفظ نسخة احتياطية', detail: 'نسخة محلية مشفّرة بحجم ٤.٢ ميجابايت', time: 'أمس، ١١:٤٠ م', tone: 'amber' },
  { title: 'تم تحديث بيانات العميل', detail: 'سارة علي — عنوان الشحن', time: 'أمس، ٩:١٥ م', tone: 'lilac' },
];

const chartValues = [4800, 3600, 4200, 3700, 5100, 4500, 2015];

function Icon({ name, size = 18 }: { name: string; size?: number }) {
  const paths: Record<string, string> = {
    wallet: 'M4 7.5A2.5 2.5 0 0 1 6.5 5H19v14H6.5A2.5 2.5 0 0 1 4 16.5v-9Zm0 1H19M15 13h.01',
    cart: 'M3 5h2l2.3 10.2a1 1 0 0 0 1 .8h8.4a1 1 0 0 0 1-.8L20 8H6M9 20a1 1 0 1 0 0-2 1 1 0 0 0 0 2Zm10 0a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z',
    users: 'M16 20v-2a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4v2M9.5 10a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7ZM17 11a3 3 0 0 0 0-6M21 20v-2a4 4 0 0 0-3-3.9',
    message: 'M20 15a3 3 0 0 1-3 3H9l-5 3V7a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v8Z',
    boxes: 'M3 8.5 12 4l9 4.5v8L12 21l-9-4.5v-8ZM3 8.5l9 4.5 9-4.5M12 13v8M7.5 6.2l9 4.5',
    chart: 'M4 20V5m0 15h17M8 16l3-5 3 2 5-8',
    warning: 'M12 3 1.8 21h20.4L12 3Zm0 6v6m0 3v.01',
    download: 'M12 3v12m0 0 4-4m-4 4-4-4M4 19h16',
    refresh: 'M20 11a8 8 0 1 1-2.3-5.7M20 4v5h-5',
    search: 'm20 20-4.2-4.2M18 10.5a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0Z',
    check: 'm5 12 4 4L19 6',
    chevron: 'm6 9 6 6 6-6',
    send: 'm3 11 18-8-8 18-2-7-8-3Z',
    clock: 'M12 7v5l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z',
    truck: 'M3 7h11v9H3zM14 10h4l3 3v3h-7zM7 19a2 2 0 1 0 0-4 2 2 0 0 0 0 4Zm11 0a2 2 0 1 0 0-4 2 2 0 0 0 0 4Z',
    package: 'M12 3 4 7v10l8 4 8-4V7l-8-4Zm-8 4 8 4 8-4M12 11v10',
    tag: 'M20 13 11 22l-9-9V2h11l9 9ZM7.5 7.5h.01',
    more: 'M5 12h.01M12 12h.01M19 12h.01',
    arrow: 'm14 6-6 6 6 6',
  };
  return (
    <svg aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d={paths[name] ?? paths.tag} />
    </svg>
  );
}

function buildChart() {
  const min = 1500;
  const max = 5500;
  const points = chartValues.map((value, index) => ({
    x: 34 + index * 95,
    y: 180 - ((value - min) / (max - min)) * 140,
  }));
  const line = points.map((point, index) => `${index === 0 ? 'M' : 'L'}${point.x} ${point.y}`).join(' ');
  const area = `${line} L${points[points.length - 1].x} 190 L${points[0].x} 190 Z`;
  return { points, line, area };
}

export default function StoreDashboard({ section, onSectionChange, onNavigate }: Props) {
  const [selectedTicket, setSelectedTicket] = useState('tk-0248');
  const [conversations, setConversations] = useState(initialConversations);
  const [search, setSearch] = useState('');
  const [reply, setReply] = useState('');
  const [orderSearch, setOrderSearch] = useState('');

  const chart = useMemo(buildChart, []);
  const conversation = conversations.find((item) => item.id === selectedTicket) ?? conversations[0];
  const filteredConversations = conversations.filter((item) =>
    `${item.name} ${item.subject} ${item.ticket} ${item.preview}`.toLowerCase().includes(search.trim().toLowerCase()),
  );
  const filteredOrders = orders.filter((order) =>
    `${order.id} ${order.customer} ${order.status}`.toLowerCase().includes(orderSearch.trim().toLowerCase()),
  );

  function sendReply(event: FormEvent) {
    event.preventDefault();
    const text = reply.trim();
    if (!text) return;
    setConversations((current) => current.map((item) => item.id === selectedTicket
      ? {
          ...item,
          preview: text,
          status: 'قيد المعالجة',
          statusTone: 'progress',
          messages: [...item.messages, { id: `m-${Date.now()}`, author: 'agent', text, time: 'الآن' }],
        }
      : item));
    setReply('');
  }

  function changeStatus(status: Conversation['status']) {
    const tone: Conversation['statusTone'] = status === 'جديد' ? 'new' : status === 'قيد المعالجة' ? 'progress' : status === 'بانتظار العميل' ? 'waiting' : 'done';
    setConversations((current) => current.map((item) => item.id === selectedTicket ? { ...item, status, statusTone: tone } : item));
  }

  if (section === 'support') {
    return (
      <div className="manager-page" dir="rtl">
        <div className="manager-heading">
          <div>
            <h1>رسائل الدعم</h1>
            <p>استمع لعملائك وتابع كل محادثة حتى الحل النهائي.</p>
          </div>
          <button type="button" className="manager-refresh" onClick={() => { setConversations(initialConversations); setSelectedTicket('tk-0248'); }}>
            <Icon name="refresh" size={16} /> تحديث
          </button>
        </div>

        <div className="support-grid">
          <section className="conversation-list" aria-label="قائمة المحادثات">
            <div className="manager-search">
              <Icon name="search" size={18} />
              <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="ابحث في المحادثات..." aria-label="ابحث في المحادثات" />
            </div>
            <div className="conversation-list-body">
              {filteredConversations.map((item) => (
                <button
                  type="button"
                  key={item.id}
                  className={`conversation-item ${item.id === selectedTicket ? 'selected' : ''}`}
                  onClick={() => setSelectedTicket(item.id)}
                >
                  <div className="conversation-item-top">
                    <strong>{item.name}</strong>
                    <span className={`manager-badge ${item.statusTone}`}>{item.status}</span>
                  </div>
                  <p>{item.preview}</p>
                  <div className="conversation-item-meta"><span>{item.ticket}</span><span>{item.date}</span></div>
                </button>
              ))}
              {!filteredConversations.length && <div className="manager-empty">لا توجد محادثات مطابقة.</div>}
            </div>
          </section>

          <section className="conversation-panel" aria-label="المحادثة المختارة">
            <header className="conversation-header">
              <div>
                <h2>{conversation.subject}</h2>
                <p>{conversation.name} · {conversation.ticket}</p>
              </div>
              <div className="conversation-header-actions">
                <button type="button" className="conversation-icon-button" title="تمت القراءة"><Icon name="check" size={16} /></button>
                <label className="status-select">
                  <span>الحالة</span>
                  <select value={conversation.status} onChange={(event) => changeStatus(event.target.value as Conversation['status'])}>
                    <option value="جديد">جديد</option>
                    <option value="قيد المعالجة">قيد المعالجة</option>
                    <option value="بانتظار العميل">بانتظار العميل</option>
                    <option value="تم الحل">تم الحل</option>
                  </select>
                  <Icon name="chevron" size={13} />
                </label>
              </div>
            </header>

            <div className="conversation-thread">
              {conversation.messages.map((message) => (
                <div key={message.id} className={`message-row ${message.author}`}>
                  {message.author === 'agent' && <div className="message-avatar">ف</div>}
                  <div className="message-body">
                    <div className="message-bubble">{message.text}</div>
                    <span className="message-time">{message.time}</span>
                  </div>
                  {message.author === 'customer' && <div className="message-avatar customer-avatar">{conversation.name.slice(0, 1)}</div>}
                </div>
              ))}
            </div>

            <form className="conversation-composer" onSubmit={sendReply}>
              <textarea value={reply} onChange={(event) => setReply(event.target.value)} placeholder="اكتب ردك على العميل..." aria-label="اكتب ردك على العميل" />
              <div className="composer-footer">
                <span>المتابعة مع العميل محفوظة محليًا</span>
                <button type="submit" className="manager-primary-button"><Icon name="send" size={15} /> إرسال الرد</button>
              </div>
            </form>
          </section>
        </div>
      </div>
    );
  }

  if (section === 'orders') {
    return (
      <div className="manager-page" dir="rtl">
        <div className="manager-heading">
          <div>
            <h1>الطلبات</h1>
            <p>تابع طلبات المتجر من المراجعة حتى التوصيل.</p>
          </div>
          <button type="button" className="manager-primary-button" onClick={() => onSectionChange('overview')}>
            <Icon name="chart" size={16} /> نظرة عامة
          </button>
        </div>
        <section className="manager-card orders-card">
          <div className="orders-toolbar">
            <div className="manager-search"><Icon name="search" size={17} /><input value={orderSearch} onChange={(event) => setOrderSearch(event.target.value)} placeholder="ابحث برقم الطلب أو اسم العميل..." aria-label="ابحث في الطلبات" /></div>
            <div className="orders-count">{filteredOrders.length} طلبات</div>
          </div>
          <div className="orders-table-wrap">
            <table className="orders-table">
              <thead><tr><th>رقم الطلب</th><th>العميل</th><th>التاريخ</th><th>الأصناف</th><th>الإجمالي</th><th>الحالة</th><th /></tr></thead>
              <tbody>
                {filteredOrders.map((order) => (
                  <tr key={order.id}>
                    <td className="mono-cell">{order.id}</td>
                    <td>{order.customer}</td>
                    <td>{order.date}</td>
                    <td>{order.items} قطع</td>
                    <td><strong>{order.total}</strong></td>
                    <td><span className={`manager-badge ${order.tone}`}>{order.status}</span></td>
                    <td><button type="button" className="row-action" onClick={() => onSectionChange('support')} title="عرض التفاصيل"><Icon name="arrow" size={16} /></button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      </div>
    );
  }

  if (section === 'activity') {
    return (
      <div className="manager-page" dir="rtl">
        <div className="manager-heading">
          <div>
            <h1>سجل النشاط</h1>
            <p>آخر التغييرات والأحداث داخل متجرك.</p>
          </div>
          <button type="button" className="manager-refresh"><Icon name="refresh" size={16} /> تحديث</button>
        </div>
        <section className="manager-card activity-card">
          {activity.map((item) => (
            <div className="activity-row" key={item.title}>
              <span className={`activity-dot ${item.tone}`}><Icon name={item.tone === 'blue' ? 'message' : item.tone === 'green' ? 'package' : item.tone === 'amber' ? 'check' : 'clock'} size={16} /></span>
              <div><strong>{item.title}</strong><p>{item.detail}</p></div>
              <time>{item.time}</time>
            </div>
          ))}
        </section>
      </div>
    );
  }

  const points = chart.points;
  return (
    <div className="manager-page" dir="rtl">
      <div className="manager-notice">
        <Icon name="warning" size={15} />
        <span><strong>معاينة لوحة الإدارة:</strong> بيانات تجريبية توضيحية — بيانات المبيعات والعملاء الحقيقية تظل محفوظة محليًا داخل الجهاز.</span>
        <button type="button" onClick={() => onNavigate('reports')}>الذهاب إلى لوحة التحكم</button>
      </div>

      <div className="manager-heading">
        <div>
          <h1>متجرك، في نظرة واحدة</h1>
          <p>تابع أداء متجرك باستمرار من خلال متابعة الطلبات والمبيعات والعملاء والرسائل.</p>
        </div>
        <div className="manager-heading-actions">
          <button type="button" className="date-chip"><Icon name="clock" size={15} /> ٢١ نوفمبر ٢٠٢٦</button>
          <button type="button" className="manager-primary-button" onClick={() => onSectionChange('orders')}><Icon name="download" size={16} /> تصدير الطلبات</button>
        </div>
      </div>

      <div className="manager-stats">
        <div className="manager-stat">
          <div className="manager-stat-icon rose"><Icon name="wallet" size={19} /></div>
          <div className="manager-stat-copy"><span>إجمالي المبيعات المحصلة</span><strong>30,900 <small>ج.م</small></strong><em>عبر 5 وسائل دفع مختلفة</em></div>
        </div>
        <div className="manager-stat">
          <div className="manager-stat-icon blue"><Icon name="cart" size={19} /></div>
          <div className="manager-stat-copy"><span>إجمالي الطلبات</span><strong>48 <small>طلب</small></strong><em>1 بالانتظار الأخير</em></div>
        </div>
        <div className="manager-stat">
          <div className="manager-stat-icon green"><Icon name="users" size={19} /></div>
          <div className="manager-stat-copy"><span>العملاء</span><strong>6 <small>عملاء</small></strong><em>حسابات العملاء النشطة</em></div>
        </div>
        <div className="manager-stat">
          <div className="manager-stat-icon amber"><Icon name="message" size={19} /></div>
          <div className="manager-stat-copy"><span>رسائل الدعم المفتوحة</span><strong>3 <small>رسائل</small></strong><em>تحتاج إلى دراسة أوفتاحية</em></div>
        </div>
      </div>

      <button type="button" className="review-alert" onClick={() => onSectionChange('orders')}>
        <Icon name="warning" size={18} />
        <span><strong>لديك 1 طلبات بانتظار المراجعة</strong> — راجع الطلبات الواردة لتأكيدها قبل التجهيز والشحن.</span>
        <span className="review-alert-action">مراجعة الطلبات <Icon name="arrow" size={15} /></span>
      </button>

      <div className="manager-charts">
        <section className="manager-card sales-card">
          <div className="sales-card-head">
            <div><h2>نظرة على المبيعات</h2><p>مجموع المبيعات المحصلة من العملاء خلال الفترة المحددة</p></div>
            <span className="range-chip">آخر 7 أيام <Icon name="chevron" size={13} /></span>
          </div>
          <div className="sales-total"><span>إجمالي المبيعات</span><strong>27,915 <small>ج.م</small></strong><i /></div>
          <div className="sales-chart">
            <svg viewBox="0 0 640 200" preserveAspectRatio="none" role="img" aria-label="رسم بياني للمبيعات خلال آخر سبعة أيام">
              <defs>
                <linearGradient id="salesFill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#e6294b" stopOpacity="0.20" />
                  <stop offset="100%" stopColor="#e6294b" stopOpacity="0.01" />
                </linearGradient>
              </defs>
              <line x1="0" y1="62" x2="640" y2="62" className="chart-grid" />
              <line x1="0" y1="120" x2="640" y2="120" className="chart-grid" />
              <line x1="0" y1="178" x2="640" y2="178" className="chart-grid" />
              <path d={chart.area} fill="url(#salesFill)" />
              <path d={chart.line} fill="none" stroke="#e11d41" strokeWidth="2.4" strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
              {points.map((point, index) => <circle key={index} cx={point.x} cy={point.y} r="4" fill="#fff" stroke="#e11d41" strokeWidth="2.4" vectorEffect="non-scaling-stroke" />)}
            </svg>
            <div className="chart-labels">
              {chartValues.map((_, index) => <span key={index}>اليوم {['١', '٢', '٣', '٤', '٥', '٦', '٧'][index]}</span>)}
            </div>
          </div>
        </section>
      </div>
        <section className="manager-card status-card">
          <div className="card-heading">
            <h2>حالة الطلبات</h2>
            <p>من الشراء إلى التوصيل</p>
          </div>
          <div className="donut-wrap">
            <div className="donut">
              <div className="donut-hole"><strong>48</strong><span>إجمالي الطلبات</span></div>
            </div>
            <div className="donut-legend">
              <div><span className="legend-dot red" /><span>تم التوصيل</span><strong>34</strong></div>
              <div><span className="legend-dot pink" /><span>قيد الشحن</span><strong>8</strong></div>
              <div><span className="legend-dot gray" /><span>قيد التجهيز</span><strong>4</strong></div>
              <div><span className="legend-dot blue" /><span>قيد المراجعة</span><strong>2</strong></div>
            </div>
          </div>
        </section>
    </div>
  );
}
