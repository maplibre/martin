// left/right arrow keys page through the docs guide
(() => {
  // don't hijack arrows while typing in the search box or a form control
  function isTypingTarget(el) {
    if (!el) {
      return false;
    }
    if (el.isContentEditable) {
      return true;
    }
    var tag = el.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
  }

  // one document-level listener survives "instant" navigation (body swapped, no full reload)
  // re-query the footer links each keypress so they track the current page
  document.addEventListener('keydown', (event) => {
    // leave modifier combos alone - e.g. browser back/forward shortcuts
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
      return;
    }
    if (isTypingTarget(document.activeElement)) {
      return;
    }

    var selector;
    if (event.key === 'ArrowLeft') {
      selector = 'a.md-footer__link--prev[href]';
    } else if (event.key === 'ArrowRight') {
      selector = 'a.md-footer__link--next[href]';
    } else {
      return;
    }

    var link = document.querySelector(selector);
    if (link && link.getAttribute('href')) {
      event.preventDefault();
      // click() so "instant" navigation can intercept it
      link.click();
    }
  });
})();
