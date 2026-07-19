(function(root, factory) {
  if (typeof module === 'object' && module.exports) {
    module.exports = factory();
  } else {
    root.codexFormatResetTime = factory();
  }
}(typeof window !== 'undefined' ? window : this, function() {
  function formatCodexResetTime(reset, timeZone) {
    if (!reset) return '';
    var date = new Date(reset);
    if (Number.isNaN(date.getTime())) return reset;

    var resolvedTimeZone = timeZone || Intl.DateTimeFormat().resolvedOptions().timeZone;
    var options = {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      second: '2-digit',
      hour12: true
    };
    if (resolvedTimeZone) options.timeZone = resolvedTimeZone;

    var parts = new Intl.DateTimeFormat('en-US', options).formatToParts(date).reduce(function(values, part) {
      values[part.type] = part.value;
      return values;
    }, {});
    var zoneLabel = resolvedTimeZone || 'Local';

    return parts.month + ' ' + parts.day + ', ' + parts.hour + ':' +
      parts.minute + ':' + parts.second + parts.dayPeriod.toLowerCase() +
      ' (' + zoneLabel + ')';
  }

  return formatCodexResetTime;
}));
