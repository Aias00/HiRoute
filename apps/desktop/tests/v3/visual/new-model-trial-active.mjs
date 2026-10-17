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
async function open(query,width=755,height=600){const url=`${base}new-model-trial.html?${query}`;await client.send('Page.enable');await client.send('Runtime.enable');await client.send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await client.send('Page.navigate',{url});await waitFor(client,`location.href===${JSON.stringify(url)} && !!document.querySelector('.models-feature,.plan-editor,.sessions-page,.trust-attention,.new-models')`);await evaluate(client,'document.fonts.ready.then(()=>true)');}
async function geometry(){return evaluate(client,`(()=>{const nodes=[...document.querySelectorAll('.model-feed-panel,.trust-control,.trust-evidence,.dialog')].filter(e=>e.getClientRects().length);return nodes.map(e=>({class:e.className,width:e.clientWidth,scroll:e.scrollWidth,left:e.getBoundingClientRect().left,right:e.getBoundingClientRect().right}));})()`);}
try {
 if(!process.env.TRIAL_QUICK_ONLY) {
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
 for (const [lang,theme,width,height] of [['en','light',755,600],['zh','dark',755,600],['en','dark',1200,813],['zh','light',1200,813]]) {
  await open(`lang=${lang}&theme=${theme}&view=timeline`,width,height);
  await waitFor(client,`document.querySelectorAll('.routing-turn').length===2`);
  await waitFor(client,`document.querySelector('.routing-turn').textContent.includes('0.40')`);
  assert.equal(await evaluate(client,`document.querySelectorAll('.routing-turn').length`),2);
  assert.ok(await evaluate(client,`trialTrace.filter(x=>x.command==='observation_read'&&x.payload.request.intent.view==='routing_timeline')[1].payload.request.intent.query.after_watermark==='watermark-1'`));
  await click('button',lang==='zh'?'紧凑视图':'Compact view');
  assert.equal(await evaluate(client,`getComputedStyle(document.querySelector('.session-master')).display`),'none');
  await evaluate(client,`document.querySelectorAll('.routing-turn')[1].click();true`);
  assert.ok(await evaluate(client,`document.querySelector('.routing-request.selected')?.textContent.includes('trial-model')`));
  for (const g of await evaluate(client,`[...document.querySelectorAll('.routing-timeline,.routing-conversation,.routing-turn-detail,.session-routing-and-conversation')].map(e=>({width:e.clientWidth,scroll:e.scrollWidth,left:e.getBoundingClientRect().left,right:e.getBoundingClientRect().right}))`)) {assert.ok(g.scroll<=g.width+1,JSON.stringify(g));assert.ok(g.left>=-1&&g.right<=width+1,JSON.stringify(g));}
  await shot(`timeline-${lang}-${theme}-${width}`);
  results.push({id:`timeline.late-score-and-request-selection.${lang}.${theme}.${width}`,status:'passed'});
 }
 await open('lang=en&view=editor&division=1',755,600);
 await click('[role="tab"]','Model performance');await waitFor(client,`!!document.querySelector('.trust-evidence-row')`);await click('summary','Level suggestions');await click('button','Review division draft');
 await waitFor(client,`!!document.querySelector('.division-draft-form') && document.querySelector('.division-draft-form').textContent.includes('DeepSeek trial')`);
 assert.equal(await evaluate(client,`document.querySelector('.division-draft-form input[type=checkbox]').checked`),false);
 await click('[role="dialog"] button','Review draft in editor');
 assert.equal(await evaluate(client,`divisionDraft.branch_routing.branches[1].primary_candidates.length`),0);
 assert.equal(await evaluate(client,`divisionDraft.branch_routing.branches[0].candidates[0].binding_id`),'binding/codex/gpt-5.6-sol');
 await evaluate(client,`document.querySelector('.division-draft-form input[type=checkbox]').click();true`);await click('[role="dialog"] button','Review draft in editor');
 assert.equal(await evaluate(client,`divisionDraft.branch_routing.branches[1].primary_candidates[0].binding_id`),'binding/codex/gpt-5.6-sol');
 await click('[role="dialog"] button','Delegate to an agent');await waitFor(client,`document.querySelector('.division-executors').textContent.includes('Dependencies ready')`);
 assert.equal(await evaluate(client,`document.querySelectorAll('.division-executors input:checked').length`),0);
 assert.ok(await evaluate(client,`document.querySelector('.division-executors label').textContent.includes('DeepSeek Harness')`));
 assert.equal(await evaluate(client,`[...document.querySelectorAll('[role="dialog"] button')].find(e=>e.textContent.includes('Review draft in editor')).disabled`),true);
 await evaluate(client,`document.querySelector('.division-executors input').click();true`);await click('[role="dialog"] button','Review draft in editor');
 assert.equal(await evaluate(client,`divisionDraft.delegation_enabled&&divisionDraft.work.harness==='deepseek_harness'&&divisionDraft.branch_routing.branches.every(b=>b.primary_candidates.length===0&&b.candidates[0].binding_id==='binding/bailian/qwen-coder')`),true);
 assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='preview_plan_editor').length`),0);
 await shot('division-worker-en-755');results.push({id:'division.optional-primary-and-explicit-executor.no-publication',status:'passed'});
 await open('lang=zh&view=attention&theme=dark');await shot('attention-zh-dark-755');await click('button','查看证据与选项');assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='review-trust').length`),1);results.push({id:'attention.optional-evidence-review',status:'passed'});
 await open('lang=en&view=attention&error=1');assert.ok(await evaluate(client,`document.querySelector('.trust-attention [role=alert]').textContent.includes('retry before deciding')`));assert.equal(await evaluate(client,`[...document.querySelectorAll('.trust-attention button')].find(e=>e.textContent.includes('Review evidence')).disabled`),true);await click('button','Retry');assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='retry-suggestions').length`),1);results.push({id:'attention.failed-read-is-not-empty',status:'passed'});
 }
 for(const [lang,theme,lost] of [['en','light',false],['zh','dark',true]]) {
  await open(`lang=${lang}&theme=${theme}&view=quickstart${lost?'&lost=1':''}`,755,600);
  await click('.new-models button',lang==='zh'?'试用':'Try');
  await waitFor(client,`!!document.querySelector('.trial-agent-choice')`);
  assert.equal(await evaluate(client,`[...document.querySelectorAll('.trial-presets input:checked')].length`),1);
  assert.equal(await evaluate(client,`document.querySelectorAll('.trial-dialog .editor-section select')[1].value`),'');
  await evaluate(client,`(()=>{const el=document.querySelectorAll('.trial-dialog .editor-section select')[1];el.value=primaryModel.model_configuration_id;el.dispatchEvent(new Event('change',{bubbles:true}));return true;})()`);
  await shot(`quickstart-setup-${lang}-${theme}`);
  await click('.trial-dialog button',lang==='zh'?'核对路由与映射':'Review route and mapping');
  await waitFor(client,`!!document.querySelector('.trial-review .trust-control')`);
  assert.ok(await evaluate(client,`document.querySelector('.trial-review .trust-levels [aria-pressed=true]').textContent.includes('0.9')`));
  await waitFor(client,`!!document.querySelector('.trial-review > .actions .btn-primary:not(:disabled)')`);
  await click('.trial-review button',lang==='zh'?'保存并发布路由':'Save and publish route');
  if(lost) {
   await waitFor(client,`trialTrace.some(x=>x.command==='preview_plan_editor'&&x.payload.input.action==='publish')`);
   await click('.trial-review button',lang==='zh'?'刷新结果':'Refresh result');
  }
  await waitFor(client,`!!document.querySelector('.trial-mapping .trial-agent-choice')`);
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='preview_plan_editor'&&x.payload.input.action==='save_draft').length`),1);
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='preview_plan_editor'&&x.payload.input.action==='publish').length`),1);
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='preview_agent_settings').length`),0);
  await click('.trial-mapping button',lang==='zh'?'检查并映射 Agent':'Review and map Agent');
  await waitFor(client,`document.querySelector('.trial-mapping').textContent.includes(${JSON.stringify(lang==='zh'?'映射已确认':'Mapping confirmed')})`);
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='preview_agent_settings').length`),1);
  assert.equal(await evaluate(client,`trialTrace.find(x=>x.command==='preview_agent_settings').payload.input.spec.model.settings.preset_mappings.sonnet.kind`),'preserve_native');
  assert.equal(await evaluate(client,`trialTrace.find(x=>x.command==='preview_agent_settings').payload.input.spec.collaboration.intent`),'keep');
  for(const g of await evaluate(client,`[...document.querySelectorAll('.trial-dialog,.trial-mapping')].map(e=>({width:e.clientWidth,scroll:e.scrollWidth,left:e.getBoundingClientRect().left,right:e.getBoundingClientRect().right}))`)) {assert.ok(g.scroll<=g.width+1,JSON.stringify(g));assert.ok(g.left>=-1&&g.right<=756,JSON.stringify(g));}
  await shot(`quickstart-complete-${lang}-${theme}`);
  results.push({id:`quickstart.${lang}.${lost?'lost-publication-response':'first-mapping'}.no-duplicate`,status:'passed'});
 }
 await writeFile(path.join(output,'report.json'),JSON.stringify({proof:'production components with synthetic IPC; no native or provider claim',results},null,2));console.log(JSON.stringify({passed:results.length,output}));
} catch(error) {await shot('failure');await writeFile(path.join(output,'failure.json'),JSON.stringify(await evaluate(client,`({text:document.body.innerText,trace:trialTrace})`),null,2));throw error;} finally {clearInterval(keepAlive);client.close();}
