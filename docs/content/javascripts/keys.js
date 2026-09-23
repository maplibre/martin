document.addEventListener('keydown', (event) => {
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
    return;
  }
  if (event.target.closest('input, textarea, select, [contenteditable]')) {
    return;
  }
  if (document.querySelector('[data-md-toggle=search]:checked')) {
    return;
  }
  const direction = { ArrowLeft: 'prev', ArrowRight: 'next' }[event.key];
  if (!direction) {
    return;
  }
  const link = document.querySelector(`.md-footer__link--${direction}`);
  if (link) {
    event.preventDefault();
    link.click();
  }
});
