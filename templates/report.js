(function () {
  "use strict";

  // "Copy selector" button in the audit details.
  var copy = document.getElementById("copy-selector");
  if (copy) {
    copy.addEventListener("click", function () {
      var message = document.getElementById("copy-message");
      var text = 'JSON.parse(document.getElementById("deburr-report").textContent)';
      if (navigator.clipboard) {
        navigator.clipboard.writeText(text).then(
          function () { message.textContent = "Copied"; },
          function () { message.textContent = text; }
        );
      } else {
        message.textContent = text;
      }
    });
  }

  // "All functions": the page holds the first rows as HTML. Every function
  // comes from the embedded JSON report, so the page stays small.
  var node = document.getElementById("deburr-report");
  var body = document.getElementById("fns");
  if (!node || !body) { return; }

  var report;
  try { report = JSON.parse(node.textContent); } catch (error) { return; }

  var linkPrefix = body.getAttribute("data-link") || "";
  var query = document.getElementById("fn-q");
  var more = document.getElementById("fn-more");
  var counter = document.getElementById("fn-count");
  var buttons = document.querySelectorAll("[data-c]");
  var step = 200;
  var shown = step;
  var category = "all";

  var functions = (report.functions || []).slice().sort(function (a, b) {
    if (b.mass !== a.mass) { return b.mass - a.mass; }
    if (a.path !== b.path) { return a.path < b.path ? -1 : 1; }
    return a.start.line - b.start.line;
  });

  function esc(text) {
    return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  function number(value) { return Number(value).toLocaleString("en-US"); }

  function place(f) {
    var text = esc(f.path) + ":" + f.start.line;
    if (!linkPrefix) { return text; }
    var path = f.path.split("/").map(encodeURIComponent).join("/");
    return '<a href="' + esc(linkPrefix + path + "#L" + f.start.line + "-L" + f.end.line) + '">' + text + "</a>";
  }

  function row(f) {
    return "<tr><td><code>" + esc(f.name) + "</code></td>" +
      '<td class="r num">' + number(Math.round(f.mass)) + "</td>" +
      '<td class="r num' + (f.high_complexity ? " over" : "") + '">' + f.cyclomatic + "</td>" +
      '<td class="r num">' + number(f.sloc) + "</td>" +
      '<td class="r num">' + f.max_nesting + "</td>" +
      '<td class="loc">' + place(f) + "</td></tr>";
  }

  function draw() {
    var needle = query.value.trim().toLowerCase();
    var matches = functions.filter(function (f) {
      if (category !== "all" && f.category !== category) { return false; }
      if (needle === "") { return true; }
      if (f.key === undefined) { f.key = (f.name + " " + f.path).toLowerCase(); }
      return f.key.indexOf(needle) >= 0;
    });
    var visible = matches.slice(0, shown);
    body.innerHTML = visible.length
      ? visible.map(row).join("")
      : '<tr><td colspan="6" class="muted">No functions match. Clear the filter to see all.</td></tr>';
    counter.textContent = "Showing " + number(visible.length) + " of " + number(matches.length) + " matching functions.";
    more.hidden = visible.length >= matches.length;
  }

  query.addEventListener("input", function () { shown = step; draw(); });
  more.addEventListener("click", function () { shown += step * 5; draw(); });
  Array.prototype.forEach.call(buttons, function (button) {
    button.addEventListener("click", function () {
      category = button.getAttribute("data-c");
      shown = step;
      Array.prototype.forEach.call(buttons, function (other) {
        other.setAttribute("aria-pressed", String(other === button));
      });
      draw();
    });
  });

  draw();
})();
