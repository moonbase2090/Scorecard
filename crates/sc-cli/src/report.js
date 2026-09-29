// SPDX-License-Identifier: MPL-2.0
// Scorecard HTML report: filters, search, copy and deep links.
// The page is complete without this script; it only adds interaction.
(function () {
  var d = document;
  d.documentElement.classList.remove("no-js");
  var rows = [].slice.call(d.querySelectorAll(".f"));
  var total = rows.reduce(function (n, r) { return n + Number(r.dataset.n); }, 0);
  var q = d.getElementById("q");
  var shown = d.getElementById("shown");

  function picked(name) {
    var on = {};
    d.querySelectorAll("input[name=" + name + "]:checked").forEach(function (i) { on[i.value] = true; });
    return on;
  }
  function keep(row, f) {
    if (!f.sev[row.dataset.sev] || !f.eng[row.dataset.eng] || !f.rule[row.dataset.rule]) return false;
    return !f.text || row.dataset.q.indexOf(f.text) >= 0;
  }
  function prune() {
    d.querySelectorAll(".rg, .fg").forEach(function (g) {
      g.hidden = !g.querySelector(".f:not([hidden])");
    });
  }
  function apply() {
    var f = { sev: picked("sev"), eng: picked("eng"), rule: picked("rule"), text: q ? q.value.trim().toLowerCase() : "" };
    var n = 0;
    rows.forEach(function (r) {
      r.hidden = !keep(r, f);
      if (!r.hidden) n += Number(r.dataset.n);
    });
    prune();
    if (shown) shown.textContent = "Showing " + n + " of " + total + " findings";
  }
  function fallbackCopy(text) {
    var t = d.createElement("textarea");
    t.value = text;
    d.body.appendChild(t);
    t.select();
    d.execCommand("copy");
    t.remove();
  }
  function copy(btn) {
    var text = btn.getAttribute("data-copy");
    if (navigator.clipboard) navigator.clipboard.writeText(text).catch(function () { fallbackCopy(text); });
    else fallbackCopy(text);
    btn.textContent = "Copied";
    setTimeout(function () { btn.textContent = "Copy"; }, 1200);
  }
  function openAll(open) {
    d.querySelectorAll("details.fg").forEach(function (g) { g.open = open; });
  }
  function reveal() {
    var id = decodeURIComponent(location.hash.slice(1));
    var el = id && d.getElementById(id);
    for (var p = el; p; p = p.parentElement) if (p.tagName === "DETAILS") p.open = true;
    if (el) el.scrollIntoView();
  }
  d.addEventListener("click", function (e) {
    var b = e.target.closest("button");
    if (!b) return;
    if (b.hasAttribute("data-copy")) copy(b);
    else if (b.dataset.act) openAll(b.dataset.act === "expand");
  });
  d.addEventListener("input", apply);
  window.addEventListener("hashchange", reveal);
  reveal();
})();
