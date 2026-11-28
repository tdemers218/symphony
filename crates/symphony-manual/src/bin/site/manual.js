// The manual's search box, the Contents button on phones, and # links on
// headings. Search runs over window.MANUAL_INDEX (search.js), built from the
// same pages; nothing leaves the browser.
(function () {
  "use strict";

  // Contents on narrow screens.
  var menu = document.querySelector(".menu");
  var toc = document.getElementById("toc");
  if (menu && toc) {
    menu.addEventListener("click", function () {
      var open = toc.classList.toggle("open");
      menu.setAttribute("aria-expanded", open ? "true" : "false");
    });
  }

  // A link on every heading, to copy.
  document.querySelectorAll("article h2[id], article h3[id]").forEach(function (h) {
    var a = document.createElement("a");
    a.className = "self";
    a.href = "#" + h.id;
    a.textContent = "#";
    a.setAttribute("aria-label", "Link to this section");
    h.appendChild(a);
  });

  var input = document.getElementById("search");
  var list = document.getElementById("results");
  var index = window.MANUAL_INDEX || [];
  if (!input || !list) return;
  var active = -1;

  function words(q) {
    return q.toLowerCase().split(/\s+/).filter(Boolean);
  }

  function score(entry, terms) {
    var title = (entry.p + " " + entry.s).toLowerCase();
    var text = entry.t.toLowerCase();
    var total = 0;
    for (var i = 0; i < terms.length; i++) {
      var t = terms[i];
      if (title.indexOf(t) >= 0) total += 10;
      else if (text.indexOf(t) >= 0) total += 1;
      else return 0;
    }
    return total;
  }

  function snippet(text, terms) {
    var lower = text.toLowerCase();
    var at = lower.indexOf(terms[0]);
    if (at < 0) at = 0;
    var start = Math.max(0, at - 40);
    var s = text.slice(start, start + 140);
    return (start > 0 ? "…" : "") + s + (start + 140 < text.length ? "…" : "");
  }

  function show() {
    var terms = words(input.value);
    list.textContent = "";
    active = -1;
    if (!terms.length) {
      list.hidden = true;
      return;
    }
    var hits = [];
    for (var i = 0; i < index.length; i++) {
      var sc = score(index[i], terms);
      if (sc) hits.push([sc, index[i]]);
    }
    hits.sort(function (a, b) { return b[0] - a[0]; });
    hits = hits.slice(0, 12);
    if (!hits.length) {
      var none = document.createElement("li");
      none.className = "none";
      none.textContent = "Nothing found. Try other words.";
      list.appendChild(none);
    }
    hits.forEach(function (h) {
      var e = h[1];
      var li = document.createElement("li");
      var a = document.createElement("a");
      a.href = e.u;
      var name = document.createElement("strong");
      name.textContent = e.s || e.p;
      a.appendChild(name);
      var where = document.createElement("span");
      where.className = "where";
      where.textContent = e.s ? e.p : "Page";
      a.appendChild(where);
      var snip = document.createElement("span");
      snip.className = "snippet";
      snip.textContent = snippet(e.t, terms);
      a.appendChild(snip);
      li.appendChild(a);
      list.appendChild(li);
    });
    list.hidden = false;
  }

  function move(step) {
    var links = list.querySelectorAll("a");
    if (!links.length) return;
    if (active >= 0) links[active].classList.remove("active");
    active = (active + step + links.length) % links.length;
    links[active].classList.add("active");
    links[active].scrollIntoView({ block: "nearest" });
  }

  input.addEventListener("input", show);
  input.addEventListener("keydown", function (e) {
    if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
    else if (e.key === "Enter") {
      var links = list.querySelectorAll("a");
      var pick = links[active >= 0 ? active : 0];
      if (pick) window.location.href = pick.href;
    } else if (e.key === "Escape") {
      input.value = "";
      show();
    }
  });
  document.addEventListener("click", function (e) {
    if (!e.target.closest(".search")) list.hidden = true;
  });
  input.addEventListener("focus", function () { if (input.value) show(); });
  // "/" focuses search, as on many documentation sites.
  document.addEventListener("keydown", function (e) {
    if (e.key === "/" && document.activeElement !== input) {
      e.preventDefault();
      input.focus();
    }
  });
})();
