// keyboard$ is the theme's key stream, the same one behind its `p` and `n` paging.
keyboard$.subscribe((key) => {
  if (key.mode !== 'global' || key.meta) {
    return;
  }
  const direction = { ArrowLeft: 'prev', ArrowRight: 'next' }[key.type];
  if (!direction) {
    return;
  }
  const link = document.querySelector(`.md-footer__link--${direction}`);
  if (link) {
    key.claim();
    link.click();
  }
});
