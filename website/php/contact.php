<?php
// REA Live Server — contact form backend
// Saves messages to php/data/messages.json (created on first message).
// With PHP inactive the site's JS falls back to a localStorage demo mode.
header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store');

// mbstring is standard in XAMPP / official Windows PHP builds; polyfill for exotic setups
if (!function_exists('mb_strlen')) {
    function mb_strlen($s) { return strlen($s ?? ''); }
}

if (($_SERVER['REQUEST_METHOD'] ?? 'GET') !== 'POST') {
    http_response_code(405);
    echo json_encode(
        ['ok' => false, 'error' => 'فقط متد POST پذیرفته می‌شود', 'method' => $_SERVER['REQUEST_METHOD'] ?? '?'],
        JSON_UNESCAPED_UNICODE
    );
    exit;
}

$name  = trim((string) ($_POST['name'] ?? ''));
$email = trim((string) ($_POST['email'] ?? ''));
$msg   = trim((string) ($_POST['message'] ?? ''));
$hp    = trim((string) ($_POST['website'] ?? '')); // honeypot — humans never fill it

$errors = [];
if (mb_strlen($name) < 2) {
    $errors[] = 'نام خیلی کوتاه است';
}
if (!filter_var($email, FILTER_VALIDATE_EMAIL)) {
    $errors[] = 'ایمیل نامعتبر است';
}
if (mb_strlen($msg) < 10) {
    $errors[] = 'پیام باید حداقل ۱۰ کاراکتر باشد';
}
if (mb_strlen($msg) > 4000) {
    $errors[] = 'پیام بیش از حد طولانی است';
}
if ($errors) {
    http_response_code(422);
    echo json_encode(['ok' => false, 'errors' => $errors], JSON_UNESCAPED_UNICODE);
    exit;
}
if ($hp !== '') {
    // bot — pretend success, store nothing
    echo json_encode(['ok' => true, 'saved' => 'پیام شما ثبت شد.'], JSON_UNESCAPED_UNICODE);
    exit;
}

$dir  = __DIR__ . '/data';
if (!is_dir($dir) && !mkdir($dir, 0775, true)) {
    http_response_code(500);
    echo json_encode(['ok' => false, 'error' => 'ساخت پوشهٔ data ناموفق بود'], JSON_UNESCAPED_UNICODE);
    exit;
}
$file = $dir . '/messages.json';

$all = [];
if (is_file($file)) {
    $all = json_decode((string) file_get_contents($file), true);
    if (!is_array($all)) {
        $all = [];
    }
}

$all[] = [
    'name'    => $name,
    'email'   => $email,
    'message' => $msg,
    'ip'      => $_SERVER['REMOTE_ADDR'] ?? '',
    'ua'      => substr((string) ($_SERVER['HTTP_USER_AGENT'] ?? ''), 0, 200),
    'time'    => date('Y-m-d H:i:s'),
];

$ok = file_put_contents(
    $file,
    json_encode($all, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES),
    LOCK_EX
) !== false;

if (!$ok) {
    http_response_code(500);
    echo json_encode(['ok' => false, 'error' => 'ذخیره‌سازی ناموفق بود'], JSON_UNESCAPED_UNICODE);
    exit;
}

echo json_encode(
    ['ok' => true, 'saved' => 'پیام شما با موفقیت ذخیره شد — ممنون!', 'count' => count($all)],
    JSON_UNESCAPED_UNICODE
);
