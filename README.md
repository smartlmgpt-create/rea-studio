# REA Studio

ویرایشگر کد سبک و سریع ساخته‌شده با **Rust + egui** - نسخهٔ 2.1.0

## دانلود (نسخهٔ v2.1.0)

| سیستم‌عامل | فایل |
|---|---|
| ویندوز (نصبی) | [REAStudio-Setup-2.1.0.exe](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-Setup-2.1.0.exe) |
| ویندوز (پرتابل، اگر موجود باشد) | [REAStudio-2.1.0.exe](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-2.1.0.exe) |
| لینوکس (نصب خودکار) | [REAStudio-2.1.0-linux-setup.sh](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-2.1.0-linux-setup.sh) |
| مک (نصب خودکار) | [REAStudio-2.1.0-macos-setup.sh](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/REAStudio-2.1.0-macos-setup.sh) |
| سورس کامل | [rea-studio-2.1.0-src.zip](https://github.com/smartlmgpt-create/rea-studio/releases/download/v2.1.0/rea-studio-2.1.0-src.zip) |

همهٔ فایل‌ها در صفحهٔ [Releases](https://github.com/smartlmgpt-create/rea-studio/releases) هستند.

## بررسی به‌روزرسانی داخل برنامه

آدرس manifest (latest.json) که برنامهٔ REA Studio می‌خواند:

    https://raw.githubusercontent.com/smartlmgpt-create/rea-studio/main/latest.json

برنامه در منوی Help -> Check for Updates این فایل را بررسی می‌کند؛ اگر نسخهٔ
جدیدتری منتشر شده باشد خبر می‌دهد. این آدرس به‌صورت خودکار در settings.json
برنامه ثبت می‌شود و از Settings هم قابل تغییر است.

## امکانات نسخهٔ 2.1.0

- منوی **Tools** با ۲۷ ابزار: مرتب‌سازی/حذف تکراری خطوط، تبدیل حروف، Base64/URL/JSON،
  هش MD5/SHA-1/SHA-256، EOL/BOM، درج زمان
- **مقایسهٔ دو فایل** (دوستونه، پرش بین تغییرات با F8)
- **اسکنر TODO** کل پوشه (TODO/FIXME/XXX/HACK) با پرش با کلیک
- **آمار سند**: خط/کلمه/نویسه/انتخاب + نشانگر LF/CRLF
- ماکروی صفحه‌کلید و انتخاب ستونی (از 2.0)
- هایلایت ۵۵+ زبان، ترمینال داخلی، LSP، پلاگین، تم تاریک/روشن
- کتاب‌نشانه‌ها، زوم (Ctrl+چرخ موس)، لیست توابع (Outline)، بازیابی نشست

## ساخت از سورس

    cargo build --release

جزئیات بیشتر در BUILDING.md و CHANGELOG-2.1.0.md