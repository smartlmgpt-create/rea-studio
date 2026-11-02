<?php
// REA Live Server — PHP health endpoint
// Returns JSON when PHP really executes; the site's JS uses this to light
// up the "PHP فعال" badge and enable real form saving.
header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store');

echo json_encode(
    [
        'php'     => true,
        'version' => PHP_VERSION,
        'sapi'    => php_sapi_name(),
        'server'  => $_SERVER['SERVER_SOFTWARE'] ?? 'unknown',
        'time'    => date('c'),
    ],
    JSON_UNESCAPED_UNICODE
);
