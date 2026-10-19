/**
 * Arabic for text produced by the server: API errors, run logs, run errors and destination
 * test reports. The server keeps English (logs stay searchable and support-friendly); the UI
 * translates on display. Unknown text is shown as is.
 *
 * Templates use placeholders:
 *   {name}       any text, kept as is
 *   {*name}      server text, translated recursively (nested errors)
 *   {,name}      comma-separated list, each item translated
 *   {name:size}  "1.2 MB"   {name:dur} "1.1s" / "3m 05s"   {name:rate} "2.0 MB/s"   {name:int} "42"
 */

const UNITS: Record<string, string> = {
  B: "بايت",
  KB: "ك.بايت",
  MB: "م.بايت",
  GB: "ج.بايت",
  TB: "ت.بايت",
  PB: "ب.بايت",
};

const SIZE = String.raw`\d+(?:\.\d+)? (?:B|KB|MB|GB|TB|PB)`;
const DURATION = String.raw`\d+(?:\.\d+)?s|\d+m \d+s|\d+h \d+m`;

function localizeSize(value: string): string {
  const match = /^(\d+(?:\.\d+)?) (B|KB|MB|GB|TB|PB)$/.exec(value);
  return match ? `${match[1]} ${UNITS[match[2]]}` : value;
}

function localizeDuration(value: string): string {
  let m = /^(\d+(?:\.\d+)?)s$/.exec(value);
  if (m) return `${m[1]} ث`;
  m = /^(\d+)m (\d+)s$/.exec(value);
  if (m) return `${m[1]} د ${m[2]} ث`;
  m = /^(\d+)h (\d+)m$/.exec(value);
  if (m) return `${m[1]} س ${m[2]} د`;
  return value;
}

function localizeRate(value: string): string {
  return value.endsWith("/s") ? `${localizeSize(value.slice(0, -2))}/ث` : value;
}

/** Exact messages. */
const EXACT: Record<string, string> = {
  // run triggers and states
  manual: "يدوي",
  schedule: "مجدول",
  backup: "النسخ الاحتياطي",
  restore: "الاستعادة",
  cancelled: "أُلغيت",
  Cancelled: "أُلغيت",
  "Cancelled by user": "ألغاها المستخدم",
  Interrupted: "توقفت",
  "Interrupted during upload": "توقفت أثناء الرفع",
  "Interrupted: the service stopped while this run was in progress": "توقفت: توقفت الخدمة أثناء تنفيذ هذه العملية",
  "Interrupted: the service was shut down while this run was in progress":
    "توقفت: أُوقفت الخدمة أثناء تنفيذ هذه العملية",
  "Not uploaded: the run was interrupted": "لم تُرفع: توقفت العملية",
  "Not uploaded: the run ended before this step": "لم تُرفع: انتهت العملية قبل هذه الخطوة",
  "Waiting for another run on this instance to finish…": "بانتظار انتهاء عملية أخرى على هذا الخادم…",
  "Waiting for a free worker slot…": "بانتظار توفر خانة تنفيذ شاغرة…",
  "Retention: disabled, all backups are kept": "سياسة الاحتفاظ معطّلة، ويُحتفظ بجميع النسخ الاحتياطية",
  "as a copy: new database UUID": "كنسخة: معرّف UUID جديد لقاعدة البيانات",
  "as a copy: new database UUID, neutralized": "كنسخة: معرّف UUID جديد، مع التحييد",
  "keeping the database UUID": "مع الإبقاء على معرّف UUID لقاعدة البيانات",
  "keeping the database UUID, neutralized": "مع الإبقاء على معرّف UUID، مع التحييد",

  // archive verification
  "PostgreSQL custom-format dump": "تفريغ PostgreSQL بالتنسيق المخصص",
  "all CRCs valid": "جميع قيم CRC سليمة",
  "archive verification failed": "فشل التحقق من الأرشيف",
  "verification task crashed": "تعطّلت مهمة التحقق",
  "the dump is truncated": "ملف التفريغ مقطوع",
  "dump.sql is empty": "ملف dump.sql فارغ",
  "dump.sql is missing from the archive: this is not an Odoo backup":
    "ملف dump.sql غير موجود في الأرشيف: هذه ليست نسخة احتياطية من Odoo",
  "the backup is not a readable ZIP archive (truncated or corrupted)":
    "النسخة الاحتياطية ليست أرشيف ZIP صالحًا للقراءة (مقطوع أو تالف)",
  "not a PostgreSQL custom-format dump": "الملف ليس تفريغ PostgreSQL بالتنسيق المخصص",
  "manifest.json is corrupted": "ملف manifest.json تالف",

  // run errors
  "the backup could not be stored on any destination": "تعذّر حفظ النسخة الاحتياطية في أي وجهة",
  "the instance no longer exists": "الخادم لم يعد موجودًا",
  "the instance of this run no longer exists": "خادم هذه العملية لم يعد موجودًا",
  "the instance has no databases": "لا توجد قواعد بيانات على الخادم",
  "loading the instance": "تحميل بيانات الخادم",
  "Odoo sent an empty backup": "أرسل Odoo نسخة احتياطية فارغة",
  "Odoo server error": "خطأ في خادم Odoo",
  "Access Denied": "تم رفض الوصول",
  "the Google Drive upload session kept expiring": "انتهت صلاحية جلسة الرفع إلى Google Drive بشكل متكرر",
  "Google Drive did not return an upload session URL": "لم يُرجع Google Drive رابط جلسة الرفع",
  "the destination has been deleted": "حُذفت الوجهة",
  "the destination holding this backup was deleted": "حُذفت الوجهة التي تحتوي هذه النسخة",
  "no source backup recorded": "لا توجد نسخة مصدر مسجّلة",
  "the source backup has no location": "النسخة المصدر بلا موقع",
  "the source backup no longer exists": "النسخة المصدر لم تعد موجودة",
  "the target instance no longer exists": "الخادم الهدف لم يعد موجودًا",
  "this schedule never fires": "هذا الجدول لا يُنفَّذ أبدًا",
  "version_info did not contain server_version": "لم تتضمن استجابة version_info قيمة server_version",
  "the URL must start with http:// or https://": "يجب أن يبدأ الرابط بـ http:// أو https://",
  "the URL has no host": "الرابط لا يحتوي على اسم خادم",

  // API errors
  "Authentication required": "يلزم تسجيل الدخول",
  "Record not found": "السجل غير موجود",
  "Instance not found": "الخادم غير موجود",
  "Job not found": "المهمة غير موجودة",
  "Destination not found": "الوجهة غير موجودة",
  "Notification channel not found": "قناة الإشعارات غير موجودة",
  "A record with the same name already exists": "يوجد سجل بالاسم نفسه",
  "Invalid username or password": "اسم المستخدم أو كلمة المرور غير صحيحة",
  "The initial setup has already been completed": "اكتمل الإعداد الأولي مسبقًا",
  "The current password is incorrect": "كلمة المرور الحالية غير صحيحة",
  "The password must be at least 10 characters long": "يجب ألا تقل كلمة المرور عن 10 أحرف",
  "The password is too long": "كلمة المرور طويلة جدًا",
  "The username must be 3-64 characters (letters, digits, '.', '_', '@', '-')":
    "يجب أن يتكون اسم المستخدم من 3 إلى 64 حرفًا (أحرف لاتينية أو أرقام أو '.' أو '_' أو '@' أو '-')",
  "The name is required (max 100 characters)": "الاسم مطلوب (100 حرف كحد أقصى)",
  "The timeout must be between 60 and 86400 seconds": "يجب أن تكون مهلة الانتظار بين 60 و86400 ثانية",
  "This instance has runs in progress; wait or cancel them first":
    "لهذا الخادم عمليات قيد التنفيذ؛ انتظر انتهاءها أو ألغِها أولًا",
  "This job has runs in progress; wait or cancel them first": "لهذه المهمة عمليات قيد التنفيذ؛ انتظر انتهاءها أو ألغِها أولًا",
  "Nothing was started: every database of this job is already being backed up":
    "لم يبدأ شيء: جميع قواعد بيانات هذه المهمة قيد النسخ الاحتياطي حاليًا",
  "The selected instance does not exist": "الخادم المحدد غير موجود",
  "The format must be 'zip' or 'dump'": "يجب أن تكون الصيغة 'zip' أو 'dump'",
  "Select at least one database (or all databases)": "اختر قاعدة بيانات واحدة على الأقل (أو جميع قواعد البيانات)",
  "Select at least one database": "اختر قاعدة بيانات واحدة على الأقل",
  "Select at least one destination": "اختر وجهة واحدة على الأقل",
  "retries must be between 0 and 10": "يجب أن يكون عدد المحاولات بين 0 و10",
  "stale_after_hours must be between 1 and 8760": "يجب أن تكون مدة التقادم بين 1 و8760 ساعة",
  "log_retention_days must be between 0 and 3650": "يجب أن تكون مدة الاحتفاظ بالسجلات بين 0 و3650 يومًا",
  "This run is not in progress": "هذه العملية ليست قيد التنفيذ",
  "Cancel the run before deleting it": "ألغِ العملية قبل حذفها",
  "No stored file": "لا يوجد ملف محفوظ",
  "The destination of this file was deleted": "حُذفت وجهة هذا الملف",
  "This file is not stored": "هذا الملف غير محفوظ",
  "This file is not available": "هذا الملف غير متاح",
  "Set the master password of this instance first": "عيّن كلمة المرور الرئيسية لهذا الخادم أولًا",
  "Set the master password of the target instance first": "عيّن كلمة المرور الرئيسية للخادم الهدف أولًا",
  "Unknown format": "صيغة غير معروفة",
  "Only backups can be restored": "يمكن استعادة النسخ الاحتياطية فقط",
  "Invalid database name: use letters, digits, '_', '-' or '.', starting with a letter or digit":
    "اسم قاعدة بيانات غير صالح: استخدم أحرفًا لاتينية أو أرقامًا أو '_' أو '-' أو '.'، على أن يبدأ بحرف أو رقم",
  "The destination type cannot be changed": "لا يمكن تغيير نوع الوجهة",
  "Upload the service account JSON key": "ارفع مفتاح JSON لحساب الخدمة",
  "A folder ID is required: service accounts must write into a Shared Drive folder":
    "معرّف المجلد مطلوب: يجب أن يكتب حساب الخدمة في مجلد ضمن Drive مشترك",
  "The OAuth client ID is required": "معرّف عميل OAuth مطلوب",
  "The OAuth client secret is required": "سر عميل OAuth مطلوب",
  "The OAuth client ID is missing": "معرّف عميل OAuth غير موجود",
  "the OAuth client ID is missing": "معرّف عميل OAuth غير موجود",
  "the OAuth client secret is missing": "سر عميل OAuth غير موجود",
  "OAuth client ID missing": "معرّف عميل OAuth غير موجود",
  "OAuth client secret missing": "سر عميل OAuth غير موجود",
  "The directory must be an absolute path, e.g. /backups": "يجب أن يكون المجلد مسارًا مطلقًا، مثل /backups",
  "Only Google Drive destinations can be connected": "يمكن ربط وجهات Google Drive فقط",
  "This destination does not use OAuth": "هذه الوجهة لا تستخدم OAuth",
  "Missing state": "معامل state مفقود",
  "Missing authorization code": "رمز التفويض مفقود",
  "This authorization link has expired; start again": "انتهت صلاحية رابط التفويض؛ ابدأ من جديد",
  "The webhook URL is required": "رابط Webhook مطلوب",
  "The webhook URL must start with https://": "يجب أن يبدأ رابط Webhook بـ https://",
  "The chat ID is required": "معرّف المحادثة مطلوب",
  "The bot token is required": "رمز البوت مطلوب",
  "SMTP host, sender and at least one recipient are required": "خادم SMTP والمرسل ومستلم واحد على الأقل مطلوبة",
  "The channel type cannot be changed": "لا يمكن تغيير نوع القناة",
  "Saved, but the server did not answer in time. Use Refresh to retry.":
    "تم الحفظ، لكن الخادم لم يستجب في الوقت المحدد. استخدم «تحديث» لإعادة المحاولة.",
  "The master password is rejected by Odoo": "رفض Odoo كلمة المرور الرئيسية",
  "webhook URL missing": "رابط Webhook مفقود",
  "bot token missing": "رمز البوت مفقود",
  "SMTP delivery failed": "فشل الإرسال عبر SMTP",
  "no service account key has been uploaded for this destination": "لم يُرفع مفتاح حساب خدمة لهذه الوجهة",
  "the service account key has no private key": "مفتاح حساب الخدمة لا يحتوي على مفتاح خاص",
  "the service account private key is invalid": "المفتاح الخاص لحساب الخدمة غير صالح",
  "the service account key is not valid JSON (expected the file downloaded from Google Cloud)":
    "مفتاح حساب الخدمة ليس JSON صالحًا (المتوقع هو الملف المُنزَّل من Google Cloud)",
  'the Google account is not connected yet: use "Connect Google account" on the destination':
    "حساب Google غير مربوط بعد: استخدم «ربط حساب Google» في الوجهة",
  "Google did not return a refresh token. Remove the app's access at myaccount.google.com/permissions and connect again.":
    "لم يُرجع Google رمز تحديث. أزل صلاحية التطبيق من myaccount.google.com/permissions ثم أعد الربط.",
  "invalid answer from the Google token endpoint": "استجابة غير صالحة من خدمة رموز Google",
  "unexpected response from the Google Drive API": "استجابة غير متوقعة من واجهة Google Drive",
  "cannot sign the service account assertion": "تعذّر توقيع طلب حساب الخدمة",
  "cannot decrypt secret: wrong encryption key or corrupted data":
    "تعذّر فك تشفير البيانات السرية: مفتاح التشفير خاطئ أو البيانات تالفة",
  "invalid Google Drive configuration": "إعدادات Google Drive غير صالحة",
  "invalid local destination configuration": "إعدادات المجلد المحلي غير صالحة",

  // destination test reports
  Authentication: "المصادقة",
  Account: "الحساب",
  "Target folder": "المجلد الهدف",
  "Storage quota": "مساحة التخزين",
  "Write test": "اختبار الكتابة",
  Directory: "المجلد",
  "Free space": "المساحة الحرة",
  unknown: "غير معروفة",
  "OAuth refresh token accepted": "تم قبول رمز التحديث (OAuth)",
  "uploaded and deleted a small test file using a resumable session":
    "رُفع ملف اختبار صغير ثم حُذف باستخدام جلسة رفع قابلة للاستئناف",
  "created, read back and deleted a test file": "أُنشئ ملف اختبار وقُرئ ثم حُذف",
  "The folder is not in a Shared Drive. Service accounts have no storage quota, so uploads to a regular My Drive folder fail with storageQuotaExceeded.":
    "المجلد ليس ضمن Drive مشترك. لا تملك حسابات الخدمة مساحة تخزين، لذا يفشل الرفع إلى مجلد عادي في My Drive بالخطأ storageQuotaExceeded.",
};

/** Templates, tried in order after the exact messages. */
const TEMPLATES: [string, string][] = [
  // hints appended to Odoo's own message (peeled off first so the message itself is translated)
  ["{*message}. Check the master password (admin_passwd).", "{*message}. تحقق من كلمة المرور الرئيسية (admin_passwd)."],
  ["{*message}. Set list_db = True in the Odoo configuration.", "{*message}. اضبط list_db = True في إعدادات Odoo."],

  // run log
  ["Queued ({*trigger})", "أُضيفت إلى قائمة الانتظار ({*trigger})"],
  ["Queued restore of {source} (run #{id:int}) from {destination}", "أُضيفت إلى قائمة الانتظار: استعادة {source} (العملية #{id}) من {destination}"],
  ['Requesting a {format} backup of "{database}" from {url}', 'طلب نسخة احتياطية بصيغة {format} لقاعدة البيانات "{database}" من {url}'],
  ["Downloaded {size:size} in {time:dur} ({rate:rate}), checksum verified", "اكتمل التنزيل: {size} في {time} ({rate})، وتم التحقق من المجموع الاختباري"],
  ["Downloaded {size:size} in {time:dur} ({rate:rate})", "اكتمل التنزيل: {size} في {time} ({rate})"],
  ["Attempt {n:int} failed: {*error}. Retrying in {s:int}s…", "فشلت المحاولة {n}: {*error}. إعادة المحاولة بعد {s} ث…"],
  ["Archive verified: {,parts}", "تم التحقق من الأرشيف: {,parts}"],
  ["{n:int} entries", "عدد العناصر: {n}"],
  ["dump.sql {size:size}", "dump.sql {size}"],
  ["{n:int} filestore files", "ملفات مخزن الملفات: {n}"],
  ["{n:int} modules", "الوحدات: {n}"],
  ["Uploading to {destination}…", "جارٍ الرفع إلى {destination}…"],
  ["Uploaded to {destination} in {time:dur} ({rate:rate}), checksum verified", "اكتمل الرفع إلى {destination} في {time} ({rate})، وتم التحقق من المجموع الاختباري"],
  ["Stored on {destination}: {path}", "حُفظت في {destination}: {path}"],
  ["Upload to {destination} failed: {*error}", "فشل الرفع إلى {destination}: {*error}"],
  ["Finished successfully in {time:dur}", "اكتملت بنجاح في {time}"],
  ["Finished with problems: {*error}", "اكتملت مع مشكلات: {*error}"],
  ["Failed: {*error}", "فشلت: {*error}"],
  ["Notification sent via {channel}", "أُرسل الإشعار عبر {channel}"],
  ["Notification via {channel} failed: {*error}", "فشل الإشعار عبر {channel}: {*error}"],
  ["Low free space in the temp directory: {size:size}", "المساحة الحرة في المجلد المؤقت منخفضة: {size}"],
  ["Retention on {destination}: kept {kept:int}, removed {removed:int} old backup(s)", "الاحتفاظ في {destination}: بقيت {kept} وحُذفت {removed} من النسخ القديمة"],
  ["Retention: cannot delete {id} on {destination}: {*error}", "الاحتفاظ: تعذّر حذف {id} من {destination}: {*error}"],
  ["Retention skipped for destination #{id:int}: {*error}", "تم تخطي الاحتفاظ للوجهة #{id}: {*error}"],
  ["Retention skipped on {destination}: {*error}", "تم تخطي الاحتفاظ في {destination}: {*error}"],
  ["Retention skipped: {*error}", "تم تخطي الاحتفاظ: {*error}"],
  ["Using the stored file {path}", "استخدام الملف المحفوظ {path}"],
  ["Downloading the backup from {destination}…", "جارٍ تنزيل النسخة الاحتياطية من {destination}…"],
  ['Restoring {size:size} into {url} as new database "{database}" ({*options})…', 'جارٍ استعادة {size} إلى {url} كقاعدة بيانات جديدة "{database}" ({*options})…'],
  ['Database "{database}" restored in {time:dur}', 'تمت استعادة قاعدة البيانات "{database}" في {time}'],

  // run and API errors
  ['no master password is configured for instance "{name}"', 'لم تُعيَّن كلمة مرور رئيسية للخادم "{name}"'],
  [
    "not enough free space in {dir}: {free:size} available, about {needed:size} needed (last backup was {last:size})",
    "لا توجد مساحة كافية في {dir}: المتاح {free}، والمطلوب نحو {needed} (حجم آخر نسخة {last})",
  ],
  ["Odoo returned HTTP {status} on {path}", "أعاد Odoo الرمز HTTP {status} على {path}"],
  ["Odoo returned HTTP {status} during restore", "أعاد Odoo الرمز HTTP {status} أثناء الاستعادة"],
  ["Odoo returned HTTP {status}", "أعاد Odoo الرمز HTTP {status}"],
  ["request to Odoo timed out ({detail})", "انتهت مهلة الطلب إلى Odoo ({detail})"],
  ["cannot connect to Odoo: {detail}", "تعذّر الاتصال بـ Odoo: {detail}"],
  ["This does not look like a reachable Odoo server: {*error}", "لا يبدو أن هذا خادم Odoo متاح: {*error}"],
  [
    "Odoo redirected the request to {target}. Use the final address (e.g. https://) as the instance URL.",
    "أعاد Odoo توجيه الطلب إلى {target}. استخدم العنوان النهائي (مثل https://) كرابط للخادم.",
  ],
  ["Odoo answered with an HTML page instead of performing the {*action}", "أعاد Odoo صفحة HTML بدلًا من تنفيذ {*action}"],
  ["Database backup error: {*message}", "خطأ في النسخ الاحتياطي لقاعدة البيانات: {*message}"],
  ["Database restore error: {*message}", "خطأ في استعادة قاعدة البيانات: {*message}"],
  ["unexpected database list: {error}", "قائمة قواعد بيانات غير متوقعة: {error}"],
  ["unexpected (non JSON-RPC) answer from {path}: {error}", "استجابة غير متوقعة (ليست JSON-RPC) من {path}: {error}"],
  ["invalid URL: {error}", "رابط غير صالح: {error}"],
  ["download interrupted after {size:size}: {error}", "انقطع التنزيل بعد {size}: {error}"],
  ["download interrupted: {error}", "انقطع التنزيل: {error}"],
  [
    "checksum mismatch after upload: local MD5 {local}, Google Drive MD5 {remote}",
    "عدم تطابق المجموع الاختباري بعد الرفع: MD5 المحلي {local}، وMD5 في Google Drive {remote}",
  ],
  [
    "checksum mismatch: the stored backup is corrupted (expected MD5 {expected}, got {actual})",
    "عدم تطابق المجموع الاختباري: النسخة المحفوظة تالفة (المتوقع MD5 {expected}، والفعلي {actual})",
  ],
  ['corrupted entry "{name}"', 'عنصر تالف "{name}"'],
  ["the source backup is no longer available on {destination}", "النسخة المصدر لم تعد متاحة في {destination}"],
  ["this copy of the backup is not available (status: {status})", "هذه النسخة غير متاحة (الحالة: {status})"],
  ["cannot list the databases: {*error}", "تعذّر جلب قائمة قواعد البيانات: {*error}"],
  ["Invalid database name '{name}'", "اسم قاعدة بيانات غير صالح '{name}'"],
  ["Destination #{id:int} does not exist", "الوجهة #{id} غير موجودة"],
  ["destination #{id:int} not found", "الوجهة #{id} غير موجودة"],
  ["{field} must be between 0 and 1000", "يجب أن تكون قيمة {field} بين 0 و1000"],
  ["This destination is used by: {jobs}. Remove it from those jobs first.", "هذه الوجهة مستخدمة في: {jobs}. أزلها من هذه المهام أولًا."],
  ["Some files could not be deleted, the run was kept: {errors}", "تعذّر حذف بعض الملفات، وأُبقي على العملية: {errors}"],
  [
    'A database named "{database}" already exists on {instance}. Restores always create a new database.',
    'توجد قاعدة بيانات باسم "{database}" على {instance}. تُنشئ الاستعادة دائمًا قاعدة بيانات جديدة.',
  ],
  ["Too many failed login attempts. Try again in {n:int} seconds.", "محاولات دخول فاشلة كثيرة. حاول مجددًا بعد {n} ثانية."],
  ["Invalid Google Drive settings: {error}", "إعدادات Google Drive غير صالحة: {error}"],
  ["Invalid local destination settings: {error}", "إعدادات المجلد المحلي غير صالحة: {error}"],
  ["Invalid settings: {error}", "إعدادات غير صالحة: {error}"],
  ["Unknown destination type '{kind}'", "نوع وجهة غير معروف '{kind}'"],
  ["Unknown channel type '{kind}'", "نوع قناة غير معروف '{kind}'"],
  ["Google returned an error: {error}", "أعاد Google خطأ: {error}"],
  ["Google authentication failed (HTTP {status}): {details}", "فشلت المصادقة مع Google (HTTP {status}): {details}"],
  ["Google Drive error (HTTP {status}): {details}", "خطأ من Google Drive (HTTP {status}): {details}"],
  ["cannot reach Google: {error}", "تعذّر الوصول إلى Google: {error}"],
  ["cannot reach the Google Drive API: {error}", "تعذّر الوصول إلى واجهة Google Drive: {error}"],
  ["cannot reach the Google token endpoint: {error}", "تعذّر الوصول إلى خدمة رموز Google: {error}"],
  ["cannot download from Google Drive: {error}", "تعذّر التنزيل من Google Drive: {error}"],
  ["request failed: {error}", "فشل الطلب: {error}"],
  ["this JSON is not a service account key (type = {kind})", "ملف JSON هذا ليس مفتاح حساب خدمة (النوع = {kind})"],
  ["invalid recipient address '{address}'", "عنوان مستلم غير صالح '{address}'"],
  ["invalid sender address '{address}'", "عنوان مرسل غير صالح '{address}'"],
  ['unknown time zone "{zone}"', 'منطقة زمنية غير معروفة "{zone}"'],
  [
    "a cron expression needs 5 fields (minute hour day-of-month month day-of-week), got {n:int}",
    "يتطلب تعبير cron خمسة حقول (الدقيقة الساعة يوم-الشهر الشهر يوم-الأسبوع)، والمُدخل يحتوي على {n}",
  ],
  ["invalid cron expression: {error}", "تعبير cron غير صالح: {error}"],
  ['the directory must be an absolute path (got "{path}")', 'يجب أن يكون المجلد مسارًا مطلقًا (القيمة الحالية "{path}")'],
  ["verification failed after copying to {dir}", "فشل التحقق بعد النسخ إلى {dir}"],
  ['invalid stored path "{path}"', 'مسار محفوظ غير صالح "{path}"'],

  // destination test reports
  ["Service account {email} impersonating {user}", "حساب الخدمة {email} بالنيابة عن {user}"],
  ["Service account {email}", "حساب الخدمة {email}"],
  ["{account}: {used:size} used of {limit:size}", "{account}: المستخدَم {used} من {limit}"],
  ["{account}: {used:size} used", "{account}: المستخدَم {used}"],
  ['"{name}" is not a folder', '"{name}" ليس مجلدًا'],
  ['no permission to add files to "{name}"', 'لا توجد صلاحية لإضافة ملفات إلى "{name}"'],
  ['"{name}" (in a Shared Drive), writable', '"{name}" (في Drive مشترك)، قابل للكتابة'],
  ['"{name}" (in My Drive), writable', '"{name}" (في My Drive)، قابل للكتابة'],
  ["only {size:size} available", "المتاح {size} فقط"],
  ["{size:size} available", "المتاح {size}"],
  ["cannot create {path}: {error}", "تعذّر إنشاء {path}: {error}"],
];

/** Explanatory sentences the server appends to errors; replaced wherever they appear. */
const SENTENCES: [string, string][] = [
  [
    ". A reverse proxy probably timed out while Odoo was preparing the archive; raise its read timeout (e.g. nginx proxy_read_timeout) for /web/database/",
    ". على الأرجح انتهت مهلة الوكيل العكسي (reverse proxy) أثناء تجهيز Odoo للأرشيف؛ ارفع مهلة القراءة (مثل proxy_read_timeout في nginx) للمسار /web/database/",
  ],
  [
    ' The refresh token was revoked or has expired: reconnect the Google account. OAuth consent screens left in "Testing" mode issue tokens that expire after 7 days; publish the app ("In production") to avoid this.',
    " أُلغي رمز التحديث أو انتهت صلاحيته: أعد ربط حساب Google. شاشات الموافقة المتروكة في وضع «Testing» تُصدر رموزًا تنتهي بعد 7 أيام؛ انشر التطبيق («In production») لتجنب ذلك.",
  ],
  [
    " Check that the key has not been deleted in Google Cloud and that the server clock is correct.",
    " تحقق من أن المفتاح لم يُحذف في Google Cloud ومن صحة ساعة الخادم.",
  ],
  [
    " Domain-wide delegation is not authorized for this service account and the Drive scope.",
    " التفويض على مستوى النطاق غير مسموح لحساب الخدمة هذا ونطاق Drive.",
  ],
  [" The OAuth client ID or secret is wrong.", " معرّف عميل OAuth أو السر غير صحيح."],
  [
    " Service accounts have no Google Drive storage of their own: use a folder inside a Shared Drive (add the service account as Content manager), or use OAuth with a regular Google account.",
    " لا تملك حسابات الخدمة مساحة تخزين خاصة في Google Drive: استخدم مجلدًا داخل Drive مشترك (وأضف حساب الخدمة بدور «مدير المحتوى»)، أو استخدم OAuth مع حساب Google عادي.",
  ],
  [" The Google Drive storage quota of this account is full.", " امتلأت مساحة تخزين Google Drive لهذا الحساب."],
  [" Check the folder ID and that the folder is shared with this account.", " تحقق من معرّف المجلد ومن مشاركته مع هذا الحساب."],
  [
    " The account needs write access to the folder (Editor or Content manager).",
    " يحتاج الحساب إلى صلاحية الكتابة في المجلد (محرر أو مدير محتوى).",
  ],
];

type Kind = "text" | "nested" | "list" | "size" | "dur" | "rate" | "int";

interface Template {
  regex: RegExp;
  kinds: Map<string, Kind>;
  output: string;
}

const PATTERNS: Record<Exclude<Kind, "nested" | "list">, string> = {
  text: "(.+?)",
  size: `(${SIZE})`,
  dur: `(${DURATION})`,
  rate: `((?:${SIZE})/s)`,
  int: String.raw`(\d+)`,
};

function compile([source, output]: [string, string]): Template & { names: string[] } {
  const names: string[] = [];
  const kinds = new Map<string, Kind>();
  let pattern = "";
  let rest = source;
  const placeholder = /\{([*,]?)(\w+)(?::(\w+))?\}/;
  for (let m = placeholder.exec(rest); m; m = placeholder.exec(rest)) {
    pattern += rest.slice(0, m.index).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const kind: Kind = m[1] === "*" ? "nested" : m[1] === "," ? "list" : ((m[3] as Kind | undefined) ?? "text");
    pattern += kind === "nested" || kind === "list" ? "(.+?)" : PATTERNS[kind];
    names.push(m[2]);
    kinds.set(m[2], kind);
    rest = rest.slice(m.index + m[0].length);
  }
  pattern += rest.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return { regex: new RegExp(`^${pattern}$`, "s"), kinds, output, names };
}

let compiled: ReturnType<typeof compile>[] | null = null;

function render(value: string, kind: Kind, depth: number): string {
  switch (kind) {
    case "nested":
      return translate(value, depth + 1);
    case "list":
      return value
        .split(", ")
        .map((item) => translate(item, depth + 1))
        .join("، ");
    case "size":
      return localizeSize(value);
    case "dur":
      return localizeDuration(value);
    case "rate":
      return localizeRate(value);
    default:
      // Names, paths and URLs: keep their punctuation with them inside right-to-left text.
      return /[A-Za-z/@\\]/.test(value) ? `\u2068${value}\u2069` : value;
  }
}

function translate(text: string, depth = 0): string {
  if (depth > 6) return text;
  const exact = EXACT[text];
  if (exact !== undefined) return exact;
  compiled ??= TEMPLATES.map(compile);
  for (const template of compiled) {
    const match = template.regex.exec(text);
    if (!match) continue;
    const values = new Map(template.names.map((name, index) => [name, match[index + 1]]));
    return template.output.replace(/\{([*,]?)(\w+)\}/g, (whole, _mark: string, name: string) => {
      const value = values.get(name);
      return value === undefined ? whole : render(value, template.kinds.get(name) ?? "text", depth);
    });
  }
  const size = localizeSize(text);
  if (size !== text) return size;
  // "context: cause" chains whose context is known.
  const colon = text.indexOf(": ");
  if (colon > 0) {
    const head = text.slice(0, colon);
    const translatedHead = EXACT[head];
    if (translatedHead !== undefined) return `${translatedHead}: ${translate(text.slice(colon + 2), depth + 1)}`;
  }
  return text;
}

export function translateServerText(text: string): string {
  let out = translate(text.trim());
  for (const [english, arabic] of SENTENCES) {
    if (out.includes(english)) out = out.replaceAll(english, arabic);
  }
  return out;
}
