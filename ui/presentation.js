/* Keep the existing moving sheen while avoiding background-position repainting. */
(() => {
  'use strict';
  const mounted=new WeakSet();
  const viewport=typeof IntersectionObserver==='function'?new IntersectionObserver(entries=>{
    for(const entry of entries)entry.target.classList.toggle('visual-outside',!entry.isIntersecting);
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
