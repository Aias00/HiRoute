import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { evaluate, navigate, waitFor } from './cdp-client.mjs';
export async function runOmarchyLandingCaptures(client, baseUrl, output) {
  const checks = [];
  for (const language of ['zh','en']) for (const width of [755,1200]) for (const theme of ['light','dark']) {
    const t=(zh,en)=>language==='zh'?zh:en;
    for (const landing of ['first-run','agent:codex','suggestion:trust-landing']) {
      const url=new URL('console-entry.html',baseUrl);
      for (const [k,v] of Object.entries({case:'launch',language,theme,landing})) url.searchParams.set(k,v);
      await navigate(client,url.href,{width,height:width===755?600:813});
      await waitFor(client,'!!document.querySelector(".sidebar")');
      let selector;
      if (landing==='first-run') {
        selector='.trial-agent-choice';
        await waitFor(client,'!!document.querySelector("[data-omarchy-target]")').catch(async error=>{await writeFile(path.join(output,'first-run-failure.json'),JSON.stringify(await evaluate(client,'({body:document.body.innerText,errors:window.consoleComponent.errors,paths:window.consoleComponent.requests.map(r=>({path:r.path,view:r.body?.request?.intent?.view}))})'),null,2));throw error;});
        assert.equal(await evaluate(client,'document.querySelector(".trial-agent-choice select").value'),'agent_codex_default');
        assert.ok(await evaluate(client,`document.querySelector('[data-omarchy-target]').textContent.includes(${JSON.stringify(t('独立 Profile','Independent profile'))})`));
        await evaluate(client,`(()=>{let e=document.querySelector('.trial-agent-choice select');Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value').set.call(e,'agent_claude_default');e.dispatchEvent(new Event('change',{bubbles:true}));return true;})()`);
        await waitFor(client,`document.querySelector('[data-omarchy-target]')?.textContent.includes(${JSON.stringify(t('仅通过 HiRoute','Only sessions started through HiRoute'))})`);
        assert.equal(await evaluate(client,'document.querySelector("[data-claude-activation-choice]")'),null);
      } else if (landing==='agent:codex') {
        selector='[data-agent-plan-list]';
        await waitFor(client,'document.querySelectorAll("[data-agent-mapped-plan]").length === 10');
        assert.equal(await evaluate(client,'document.querySelector("[role=dialog]")'),null);
        assert.ok(await evaluate(client,'document.querySelector("[data-agent-plan-list]").textContent.includes("<Plan label> & extra")'));
        assert.equal(await evaluate(client,'document.querySelector("[data-agent-plan-list] img")'),null);
        assert.ok(await evaluate(client,'document.querySelector(".native-list .list-row.active, .native-list .list-row.selected")?.textContent.includes("Codex") ?? document.querySelector("[data-agent-id=agent_codex_default]")?.textContent.includes("Codex")'));
      } else {
        selector='.quality-performance-panel';
        await waitFor(client,`Array.from(document.querySelectorAll('[role=tab][aria-selected=true]')).some(e=>e.textContent===${JSON.stringify(t('模型表现','Model performance'))})`);
        await waitFor(client,'!!document.querySelector(".trust-suggestion")');
        assert.ok(await evaluate(client,'window.consoleComponent.requests.some(r=>r.body?.request?.intent?.view==="plan_trust" && r.body.request.intent.query.plan_id==="plan/daily-coding")'));
      }
      assert.equal(await evaluate(client,'window.consoleComponent.requests.filter(r=>r.path.endsWith("/launch")).length'),1);
      assert.equal(await evaluate(client,'location.pathname'),'/');
      assert.equal(await evaluate(client,'location.search'),'');
      assert.deepEqual(await evaluate(client,'window.consoleComponent.errors'),[]);
      assert.ok(await evaluate(client,'!window.consoleComponent.requests.some(r=>/preview_|check_agent_live|check_subscription|apply_/.test(r.path))'),'Landing navigation must not publish or invoke a model');
      await evaluate(client,`Promise.all(document.getAnimations().filter(a=>Number.isFinite(a.effect?.getTiming().iterations)).map(a=>a.finished.catch(()=>{}))).then(()=>true)`);
      const bounds=await evaluate(client,`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('Missing settled view');e.scrollIntoView({block:'start'});const r=e.getBoundingClientRect();return {width:e.clientWidth,scroll:e.scrollWidth,left:r.left,right:r.right};})()`);
      assert.ok(bounds.scroll<=bounds.width+1 && bounds.left>=-1 && bounds.right<=width+1,JSON.stringify(bounds));
      assert.ok(await evaluate(client,'(()=>{const t=document.querySelector(".window-title .page-label"),h=document.querySelector(".titlebar");return t.getBoundingClientRect().height<=h.clientHeight && t.scrollWidth<=t.clientWidth+1;})()'),'The console title must remain on one visible line');
      if (landing==='agent:codex') assert.ok(await evaluate(client,'document.querySelector("[data-agent-plan-list] h4").getBoundingClientRect().top >= document.querySelector(".oc-model-back").getBoundingClientRect().bottom - 1'),'Mapped list heading must not sit underneath its sticky navigation');
      const name=`omarchy-landing-${landing.split(':')[0]}-${language}-${width}-${theme}.png`;
      const shot=await client.send('Page.captureScreenshot',{format:'png'});await writeFile(path.join(output,name),Buffer.from(shot.data,'base64'));
      checks.push({id:`${landing}.${language}.${width}.${theme}`,state:'green',screenshot:name});
    }
  }
  assert.equal(checks.length,24);
  await writeFile(path.join(output,'omarchy-landing-report.json'),JSON.stringify({evidence:'Production ConsoleRoot/DesktopApp/styles, synthetic authenticated transport; no daemon/model claim',checks,summary:{total:checks.length,green:checks.length,red:0}},null,2)+'\n');
}
