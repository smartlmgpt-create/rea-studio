راهنمای نصب گواهی REA Studio (برای حذف هشدار ویندوز)
=====================================================

فایل‌های 1.7.4 با گواهی «REA Studio» امضا شده‌اند. برای اینکه ویندوز
ناشر را بشناسد و هشدار ندهد، دو بار (فقط یک‌بار در هر سیستم) این کار
را انجام دهید:

گام ۱) روی فایل REAStudio-Certificate.cer دوبار کلیک کنید
گام ۲) دکمه «Install Certificate...» را بزنید
گام ۳) «Local Machine» را انتخاب کنید → Next
گام ۴) گزینه «Place all certificates in the following store» را بزنید
       و با Browse پوشه «Trusted Root Certification Authorities» را
       انتخاب کنید → Next → Finish
گام ۵) دوباره از گام ۱ شروع کنید ولی این‌بار پوشه
       «Trusted Publishers» را انتخاب کنید → Finish

نتیجه: در Properties فایل exe، تب Digital Signatures امضای
«REA Studio» را معتبر نشان می‌دهد و هشدار «Unknown Publisher» حذف
می‌شود.

توجه:
- ممکن است SmartScreen برای فایل‌های کم‌دانلود یک‌بار هشدار
  «More info → Run anyway» بدهد؛ این به سابقه دانلود برمی‌گردد نه
  به اعتبار امضا.
- کلید خصوصی این گواهی همراه فایل‌ها نیست (نکته امنیتی عمدی).
- برای امضای انتشارهای رسمی عمومی، گواهی تجاری (OV/EV) لازم است.
