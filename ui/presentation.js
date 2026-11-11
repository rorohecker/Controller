/* Theme choice (auto / light / dark / contrast). External script so it also works under the app CSP. */
(() => {
  'use strict';
  const KEY = 'controller.theme.v1', CHOICES = ['auto', 'light', 'dark', 'contrast'];
  const media = matchMedia('(prefers-color-scheme: dark)');
  let choice = 'auto';
  try { const v = localStorage.getItem(KEY); if (CHOICES.includes(v)) choice = v; } catch {}
  const resolve = () => choice === 'auto' ? (media.matches ? 'dark' : 'light') : choice;
  function apply() {
    const theme = resolve();
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.themeChoice = choice;
    document.dispatchEvent(new CustomEvent('controller-theme', { detail: { choice, theme } }));
  }
  media.addEventListener?.('change', () => { if (choice === 'auto') apply(); });
  window.controllerTheme = {
    choices: CHOICES,
    get choice() { return choice; },
    get theme() { return resolve(); },
    set(next) {
      if (!CHOICES.includes(next)) return;
      choice = next;
      try { localStorage.setItem(KEY, next); } catch {}
      apply();
    },
    /* Read a CSS custom property so canvas code follows the active theme. */
    token(name, fallback) {
      const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
      return value || fallback;
    }
  };
  apply();
})();

/* Keep the existing moving sheen while avoiding background-position repainting. */
(() => {
  'use strict';
  const mounted=new WeakSet();
  const viewport=typeof IntersectionObserver==='function'?new IntersectionObserver(entries=>{
    for(const entry of entries){
      /* A box inside a hidden page has no layout: leave its class alone so hidden pages never mutate (it is re-evaluated when the page is shown). */
      const r=entry.boundingClientRect;if(!entry.isIntersecting&&r.width===0&&r.height===0)continue;
      entry.target.classList.toggle('visual-outside',!entry.isIntersecting);
    }
  },{rootMargin:'80px'}):null;
  function mount(root){
    const boxes=[...(root.matches?.('.framed')?[root]:[]),...root.querySelectorAll('.framed')];
    for(const box of boxes){
      if(mounted.has(box)){viewport?.observe(box);continue;}mounted.add(box);
      const reflection=document.createElement('span');reflection.className='card-reflection';reflection.setAttribute('aria-hidden','true');box.append(reflection);
      viewport?.observe(box);
    }
  }
  mount(document.body);
  new MutationObserver(records=>{
    for(const record of records){
      for(const node of record.removedNodes)if(node.nodeType===1&&!node.isConnected){if(node.matches('.framed'))viewport?.unobserve(node);for(const box of node.querySelectorAll('.framed'))viewport?.unobserve(box);}
      for(const node of record.addedNodes)if(node.nodeType===1)mount(node);
    }
  }).observe(document.body,{childList:true,subtree:true});
  const visibility=()=>document.body.classList.toggle('presentation-paused',document.hidden);
  document.addEventListener('visibilitychange',visibility);visibility();
})();
