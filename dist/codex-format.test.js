const assert = require('node:assert/strict');
const test = require('node:test');
const formatCodexResetTime = require('./codex-format.js');

test('formats an absolute reset timestamp in the requested local timezone', () => {
  assert.equal(
    formatCodexResetTime('2026-07-25T21:10:12Z', 'Asia/Jakarta'),
    'Jul 26, 4:10:12am (Asia/Jakarta)'
  );
});

test('preserves missing or invalid reset values', () => {
  assert.equal(formatCodexResetTime('', 'Asia/Jakarta'), '');
  assert.equal(formatCodexResetTime('not a timestamp', 'Asia/Jakarta'), 'not a timestamp');
});
