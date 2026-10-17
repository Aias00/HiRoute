import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { connectPage, evaluate, waitFor } from './cdp-client.mjs';
const [port,base,output]=process.argv.slice(2);
const client=await connectPage(Number(port));
const keepAlive=setInterval(()=>{},1000);
const results=[];
const click=async(selector,text)=>{ await evaluate(client,`(()=>{const el=[...document.querySelectorAll(${JSON.stringify(selector)})].find(e=>e.getClientRects().length && !e.disabled && e.textContent.includes(${JSON.stringify(text)}));if(!el)throw Error('Missing control');el.click();return true;})()`); };
const shot=async name=>{const value=await client.send('Page.captureScreenshot',{format:'png'});await writeFile(path.join(output,`${name}.png`),Buffer.from(value.data,'base64'));};
async function open(query,width=755,height=600){const url=`${base}new-model-trial.html?${query}`;await client.send('Page.enable');await client.send('Runtime.enable');await client.send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await client.send('Page.navigate',{url});await waitFor(client,`location.href===${JSON.stringify(url)} && !!document.querySelector('.models-feature,.plan-editor')`);await evaluate(client,'document.fonts.ready.then(()=>true)');}
async function geometry(){return evaluate(client,`(()=>{const nodes=[...document.querySelectorAll('.model-feed-panel,.trust-control,.trust-evidence,.dialog')].filter(e=>e.getClientRects().length);return nodes.map(e=>({class:e.className,width:e.clientWidth,scroll:e.scrollWidth,left:e.getBoundingClientRect().left,right:e.getBoundingClientRect().right}));})()`);}
try {
 for(const [lang,theme,width,height] of [['en','light',755,600],['zh','dark',755,600],['en','dark',1200,813],['zh','light',1200,813]]){
  await open(`lang=${lang}&theme=${theme}`,width,height);
  await click('summary',lang==='zh'?'模型信息更新':'Model feed');
  await waitFor(client,`!!document.querySelector('.model-feed-actions')`);
  await shot(`feed-${lang}-${theme}-${width}`);
  for(const g of await geometry()){assert.ok(g.scroll<=g.width+1,JSON.stringify(g));assert.ok(g.left>=-1 && g.right<=width+1,JSON.stringify(g));}
  await click('button',lang==='zh'?'移除副本…':'Remove copy…');
  await waitFor(client,`!!document.querySelector('[role="dialog"]')`);
  assert.equal(await evaluate(client,`document.querySelector('.model-feed-checkbox input').checked`),true);
  await shot(`feed-removal-${lang}-${theme}-${width}`);
  await click('[role="dialog"] button',lang==='zh'?'取消':'Cancel');
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='model_feed_remove_apply').length`),0);
  results.push({id:`feed.review.${lang}.${theme}.${width}`,status:'passed'});
 }
 await open('lang=en&stale=1');await click('summary','Model feed');await click('button','Remove copy…');await waitFor(client,`!!document.querySelector('[role="dialog"]')`);await click('[role="dialog"] button','Remove copy');await waitFor(client,`document.body.textContent.includes('Review the impact again')`);assert.equal(await evaluate(client,`document.querySelector('[role="dialog"]')===null`),true);results.push({id:'feed.stale-removal.repreview',status:'passed'});
 await open('lang=en&view=editor&empty=1');await waitFor(client,`!!document.querySelector('[data-trust-control="smart"]')`);assert.equal(await evaluate(client,`[...document.querySelectorAll('.trust-levels button')].some(e=>e.getAttribute('aria-pressed')==='true')`),false);assert.ok(await evaluate(client,`!!document.querySelector('.trust-control').textContent.includes('Custom 0.72')`));assert.ok(await evaluate(client,`!!document.querySelector('.trust-control').textContent.includes('No recorded evidence')`));await evaluate(client,`document.querySelector('.trust-control').scrollIntoView({block:'center'});true`);await shot('trust-custom-empty-en-755');await click('.trust-levels button','Trusted');assert.ok(await evaluate(client,`!!document.querySelector('.trust-control').textContent.includes('0.65')`));assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='preview_plan_editor').length`),0);results.push({id:'trust.custom-threshold.explicit-change',status:'passed'});
 await open('lang=zh&view=editor&theme=dark',1200,813);await waitFor(client,`!!document.querySelector('[data-trust-control="smart"]')`);await click('[role="tab"]','模型表现');await waitFor(client,`!!document.querySelector('.trust-evidence-row')`);await shot('trust-evidence-zh-dark-1200');for(const g of await geometry())assert.ok(g.scroll<=g.width+1,JSON.stringify(g));assert.ok(await evaluate(client,`!!document.querySelector('.trust-evidence').textContent.includes('41 完整评分')`));results.push({id:'trust.coverage-and-version-disclosure',status:'passed'});
 await writeFile(path.join(output,'report.json'),JSON.stringify({proof:'production components with synthetic IPC; no native or provider claim',results},null,2));console.log(JSON.stringify({passed:results.length,output}));
} finally {clearInterval(keepAlive);client.close();}
