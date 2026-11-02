<?php
// REA Live Server — update-check endpoint (simulated release feed)
header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store');

$latest = [
    'version'   => '1.7.5',
    'build'     => '2026-10-31',
    'channel'   => 'stable',
    'size_mb'   => 6.5,
    'highlights' => [
        'پشتیبانی PHP در Live Server از طریق php-cgi',
        'خواندن کامل بدنهٔ درخواست‌های POST',
        'صفحهٔ راهنمای فارسی در نبود php-cgi',
    ],
    'download'  => 'https://gofile.io/d/dersubYO',
];

// optional: /php/version.php?from=1.7.2 → فقط اگر قدیمی‌تر بود
$from = $_GET['from'] ?? null;
if ($from !== null) {
    $from = preg_replace('/[^0-9.]/', '', (string) $from);
    $isNewer = version_compare($from, $latest['version'], '<');
    echo json_encode(
        ['update_available' => $isNewer] + $latest,
        JSON_UNESCAPED_UNICODE
    );
    exit;
}

echo json_encode($latest, JSON_UNESCAPED_UNICODE);
