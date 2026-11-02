/* REA Studio demo site — theme, reveal, PHP status, contact form */
(function () {
  "use strict";

  /* ---------- theme ---------- */
  var root = document.documentElement;
  var saved = null;
  try { saved = localStorage.getItem("rea-theme"); } catch (e) {}
  if (saved === "light" || saved === "dark") root.setAttribute("data-theme", saved);
  var tbtn = document.getElementById("themeToggle");
  if (tbtn) {
    var paint = function () {
      tbtn.textContent = root.getAttribute("data-theme") === "light" ? "🌙" : "☀️";
    };
    paint();
    tbtn.addEventListener("click", function () {
      var next = root.getAttribute("data-theme") === "light" ? "dark" : "light";
      root.setAttribute("data-theme", next);
      try { localStorage.setItem("rea-theme", next); } catch (e) {}
      paint();
    });
  }

  /* ---------- reveal on scroll ---------- */
  var els = document.querySelectorAll(".reveal");
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) { en.target.classList.add("in"); io.unobserve(en.target); }
      });
    }, { threshold: 0.12 });
    els.forEach(function (el) { io.observe(el); });
  } else {
    els.forEach(function (el) { el.classList.add("in"); });
  }

  /* ---------- footer year ---------- */
  var y = document.getElementById("year");
  if (y) y.textContent = String(new Date().getFullYear());

  /* ---------- PHP status badge ---------- */
  function setBadge(state, text) {
    var b = document.getElementById("phpBadge");
    if (!b) return;
    b.classList.remove("on", "off");
    if (state === "on") { b.classList.add("on"); b.textContent = "🐘 PHP فعال — " + text; }
    else { b.classList.add("off"); b.textContent = "🐘 PHP خاموش — " + text; }
    b.title = "وضعیت php-cgi روی Live Server";
  }
  fetch("php/health.php", { cache: "no-store" })
    .then(function (r) {
      var ct = (r.headers.get("content-type") || "").toLowerCase();
      return r.text().then(function (t) { return { ct: ct, t: t }; });
    })
    .then(function (o) {
      var isJson = o.ct.indexOf("json") !== -1 || /^\s*\{/.test(o.t);
      if (isJson) {
        try {
          var j = JSON.parse(o.t);
          if (j && j.php === true) { setBadge("on", "PHP " + j.version); return; }
        } catch (e) {}
      }
      setBadge("off", "فایل استاتیک سرو می‌شود");
    })
    .catch(function () { setBadge("off", "سرور در دسترس نیست"); });

  /* ---------- contact form ---------- */
  var form = document.getElementById("contactForm");
  if (form) {
    var box = document.getElementById("formMsg");
    var show = function (cls, html) {
      box.className = "msg " + cls;
      box.innerHTML = html;
    };
    form.addEventListener("submit", function (ev) {
      ev.preventDefault();
      show("", "");
      var fd = new FormData(form);
      fetch(form.getAttribute("action") || "php/contact.php", {
        method: "POST",
        body: fd,
        cache: "no-store",
      })
        .then(function (r) { return r.text().then(function (t) { return { ct: r.headers.get("content-type") || "", t: t }; }); })
        .then(function (o) {
          var isJson = o.ct.indexOf("json") !== -1 || /^\s*\{/.test(o.t);
          if (isJson) {
            try {
              var j = JSON.parse(o.t);
              if (j.ok) { show("ok", "✅ " + (j.saved || "پیام شما ذخیره شد.") + " <span class='note'>(PHP واقعی)</span>"); return; }
              show("err", "⚠️ " + ((j.errors || []).join(" • ") || j.error || "خطا")); 
              return;
            } catch (e) {}
          }
          // PHP not executing → demo mode (localStorage)
          try {
            var k = "rea-demo-messages";
            var arr = JSON.parse(localStorage.getItem(k) || "[]");
            var rec = {};
            ["name", "email", "message"].forEach(function (f) { rec[f] = String(fd.get(f) || ""); });
            rec.time = new Date().toLocaleString("fa-IR");
            arr.push(rec);
            localStorage.setItem(k, JSON.stringify(arr));
          } catch (e) {}
          show("ok", "🧪 <b>حالت نمایشی:</b> PHP روی سرور فعال نیست (پیام در مرورگر ذخیره شد). برای ذخیرهٔ واقعی، <code>php-cgi</code> را نصب و Live Server را ری‌استارت کنید.");
        })
        .catch(function () {
          show("err", "⚠️ ارتباط با سرور برقرار نشد.");
        });
    });
  }
})();
