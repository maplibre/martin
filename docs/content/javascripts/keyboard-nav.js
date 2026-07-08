// Keyboard navigation for the documentation guide.
//
// Left/Right arrow keys page through the guide, following the same
// previous/next links that are shown in the page footer. This mirrors the
// common "prev/next" behavior of left/right paged sites.
//
// See https://github.com/maplibre/martin/issues/2788
(() => {
  // Don't hijack the arrow keys while the user is typing (e.g. in the search
  // box) or interacting with a form control.
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

  // A single document-level listener keeps working across "instant" navigation,
  // where the page body is swapped without a full reload. The footer links are
  // looked up on each key press so they always point at the current page.
  document.addEventListener('keydown', (event) => {
    // Leave modifier combinations alone (e.g. browser back/forward shortcuts).
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
      // Use click() so "instant" navigation (if enabled) can intercept it.
      link.click();
    }
  });
})();
