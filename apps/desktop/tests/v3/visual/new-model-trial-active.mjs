import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { connectPage, evaluate, waitFor } from './cdp-client.mjs';
const [port,base,output]=process.argv.slice(2);
const client=await connectPage(Number(port));
const keepAlive=setInterval(()=>{},1000);
const results=[];
const click=async(selector,text)=>{ await evaluate(client,`(()=>{const el=[...document.querySelectorAll(${JSON.stringify(selector)})].find(e=>e.getClientRects().length && !e.disabled && e.textContent.includes(${JSON.stringify(text)}));if(!el)throw Error('Missing control');el.click();return true;})()`); };
const shot=async name=>{await evaluate(client,`Promise.all(document.getAnimations().filter(a=>a.effect?.getComputedTiming().iterations!==Infinity).map(a=>a.finished.catch(()=>{}))).then(()=>true)`);const value=await client.send('Page.captureScreenshot',{format:'png'});await writeFile(path.join(output,`${name}.png`),Buffer.from(value.data,'base64'));};
async function open(query,width=755,height=600){const url=`${base}new-model-trial.html?${query}`;await client.send('Page.enable');await client.send('Runtime.enable');await client.send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await client.send('Page.navigate',{url});await waitFor(client,`location.href===${JSON.stringify(url)} && !!document.querySelector('.models-feature,.plan-editor,.sessions-page,.trust-attention,.new-models')`);await evaluate(client,'document.fonts.ready.then(()=>true)');}
async function geometry(){return evaluate(client,`(()=>{const nodes=[...document.querySelectorAll('.model-feed-panel,.trust-control,.plan-quality,.trust-suggestion,.modal,.trial-wizard')].filter(e=>e.getClientRects().length);return nodes.map(e=>({class:e.className,width:e.clientWidth,scroll:e.scrollWidth,left:e.getBoundingClientRect().left,right:e.getBoundingClientRect().right}));})()`);}
try {
 for(const lang of ['en','zh']) {
  await open(`lang=${lang}&view=handoff`,755,600);
  await waitFor(client,`trialTrace.some(x=>x.command==='observation_read' && x.payload.request.intent.view==='sessions')`);
  assert.equal(await evaluate(client,`trialTrace.find(x=>x.command==='observation_read').payload.request.intent.query.from_ms===readCostState.confirmed`),true);
  assert.equal(await evaluate(client,`trialTrace.find(x=>x.command==='observation_read').payload.request.intent.query.plan_id`),'plan/trial');
  await shot(`handoff-waiting-${lang}-755`);
  await evaluate(client,`hideReadCost(true);tickReadCost(5000);true`);
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='observation_read').length`),1);
  await evaluate(client,`readCostState.fail=true;hideReadCost(false);true`);
  await waitFor(client,`!!document.querySelector('.trial-session-handoff [role=alert]')`);
  await shot(`handoff-error-${lang}-755`);
  await evaluate(client,`readCostState.fail=false;readCostState.found=true;true`);
  await click('.trial-session-handoff button',lang==='zh'?'重试':'Retry');
  await waitFor(client,`!!document.querySelector('.trial-session-handoff .btn-primary')`);
  await shot(`handoff-found-${lang}-755`);
  const foundReads=await evaluate(client,`trialTrace.filter(x=>x.command==='observation_read').length`);
  await evaluate(client,`tickReadCost(5000);true`);
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='observation_read').length`),foundReads);
  await click('.trial-session-handoff button',lang==='zh'?'观察此会话':'Watch this session');
  await waitFor(client,`trialTrace.some(x=>x.command==='observation_read' && x.payload.request.intent.view==='routing_timeline' && x.payload.request.intent.query.session_id==='trial-found')`);
  results.push({id:`handoff.wait-hide-error-found-navigate.${lang}`,status:'passed'});
 }
 await open('lang=en&view=handoff');await waitFor(client,`trialTrace.some(x=>x.command==='observation_read')`);await click('button','Close waiting');
 const closedReads=await evaluate(client,`trialTrace.length`);await evaluate(client,`tickReadCost(5000);true`);assert.equal(await evaluate(client,`trialTrace.length`),closedReads);
 results.push({id:'handoff.close-stops-reads',status:'passed'});
 await open('lang=en&view=suggestion-cadence');await waitFor(client,`!!document.querySelector('.trust-attention-item')`);
 const suggestionReads=()=>evaluate(client,`trialTrace.filter(x=>x.command==='observation_read' && x.payload.request.intent.view==='plan_trust_suggestions').length`);
 assert.equal(await suggestionReads(),1);await evaluate(client,`tickReadCost(30000);true`);assert.equal(await suggestionReads(),1);
 await evaluate(client,`tickReadCost(300000);true`);await waitFor(client,`trialTrace.length===2`);assert.equal(await suggestionReads(),2);
 await click('button','Publish fixture');await waitFor(client,`trialTrace.length===3`);assert.equal(await suggestionReads(),3);
 await click('button','Toggle Home');assert.equal(await suggestionReads(),3);await click('button','Toggle Home');await waitFor(client,`trialTrace.length===4`);
 await evaluate(client,`hideReadCost(true);tickReadCost(300000);true`);assert.equal(await suggestionReads(),4);await evaluate(client,`hideReadCost(false);true`);await waitFor(client,`trialTrace.length===5`);
 assert.ok(await evaluate(client,`document.querySelector('.trust-attention [role=alert]').textContent.includes('Other suggestions remain available')`));
 assert.equal(await evaluate(client,`document.querySelector('.trust-attention-item button').disabled`),false);await shot('suggestion-partial-failure-en-755');
 results.push({id:'suggestions.five-minute-cadence-publish-home-visible-partial-failure',status:'passed'});
 if(!process.env.TRIAL_QUICK_ONLY) {
 for(const lang of ['en','zh']) {
  await open(`lang=${lang}&view=feed-home`); await waitFor(client,`!!document.querySelector('.model-feed-disclosure')`);
  assert.ok(await evaluate(client,`document.querySelector('.model-feed-disclosure').textContent.includes('hiroute.ai')`));
  await shot(`feed-first-home-${lang}-755`);
  await click('.model-feed-disclosure button',lang==='zh'?'关闭自动获取':'Turn off automatic retrieval');
  await waitFor(client,`!document.querySelector('.model-feed-disclosure')`);
  assert.equal(await evaluate(client,`trialTrace.find(x=>x.command==='model_feed_settings_apply').payload.input.retrieval_enabled`),false);
  results.push({id:`feed.first-home-disclosure.${lang}`,status:'passed'});
 }
 await open('lang=en&recovery=1'); await waitFor(client,`document.body.textContent.includes('Saved feed state is damaged') && !!document.querySelector('.model-feed-details')`);
 await click('button','Reset damaged cache…'); await waitFor(client,`!!document.querySelector('[role=dialog]')`); await shot('feed-reset-damaged-cache-en-755');
 assert.ok(await evaluate(client,`document.querySelector('[role=dialog]').textContent.includes('damaged state file will be preserved')`));
 await click('[role=dialog] button','Confirm reset'); await waitFor(client,`!document.querySelector('[role=dialog]')`);
 assert.equal(await evaluate(client,`[...document.querySelectorAll('.model-feed-actions button')].find(b=>b.textContent.includes('Refresh')).disabled`),false);
 results.push({id:'feed.confirmed-corrupt-state-recovery',status:'passed'});
 await open('lang=en&view=timeline&scope-error=1'); await waitFor(client,`!!document.querySelector('.routing-timeline [role=alert]')`);
 const scopeReads=await evaluate(client,`trialTrace.filter(x=>x.command==='observation_read'&&x.payload.request.intent.view==='routing_timeline').length`);
 await evaluate(client,`refreshTrialTimeline();true`); await evaluate(client,`hideTrialTimeline(true);true`); await evaluate(client,`hideTrialTimeline(false);true`);
 await new Promise(resolve=>setTimeout(resolve,2200));
 assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='observation_read'&&x.payload.request.intent.view==='routing_timeline').length`),scopeReads);
 await click('.routing-timeline button','Retry'); await waitFor(client,`trialTrace.filter(x=>x.command==='observation_read'&&x.payload.request.intent.view==='routing_timeline').length===${scopeReads+1}`);
 results.push({id:'timeline.scope-error-stops-until-explicit-retry',status:'passed'});
 await open('lang=en&view=timeline&busy-read=1'); await waitFor(client,`document.querySelectorAll('.routing-turn').length===2`);
 assert.equal(await evaluate(client,`document.querySelector('.routing-timeline [role=alert]')`),null);
 assert.ok(await evaluate(client,`(()=>{const calls=trialTrace.filter(x=>x.command==='observation_read'&&x.payload.request.intent.view==='routing_timeline');return calls[1].at-calls[0].at<1000;})()`));
 results.push({id:'timeline.busy-retries-quietly',status:'passed'});
 await open('lang=en&view=editor&cost=1'); await waitFor(client,`!!document.querySelector('[data-trust-control="smart"]')`); await click('[role=tab]','Model performance');
 await waitFor(client,`document.body.textContent.includes('Additional cost')`); await evaluate(client,`document.querySelector('[aria-label="Regular cost comparison"]').scrollIntoView({block:'center'});true`); await shot('trust-cost-positive-en-755');
 assert.ok(await evaluate(client,`document.querySelector('[aria-label="Regular cost comparison"]').textContent.includes('+')`));
 results.push({id:'trust.cost-positive-difference-disclosure',status:'passed'});
 for (const lang of ['en','zh']) for (const width of [755,1200]) for (const kind of ['partial','unknown']) {
  await open(`lang=${lang}&view=editor&cost-${kind}=1`,width,width===755?600:813);
  await waitFor(client,`!!document.querySelector('[data-trust-control="smart"]')`);
  await click('[role=tab]',lang==='zh'?'模型表现':'Model performance');
  const label=lang==='zh'?'常规组成本对比':'Regular cost comparison';
  await waitFor(client,`!!document.querySelector('[aria-label="${label}"]')`);
  await evaluate(client,`(()=>{document.querySelector('.occami-root')?.setAttribute('data-theme',${JSON.stringify(width===755?'light':'dark')});document.querySelector('[aria-label="${label}"]').scrollIntoView({block:'center'});return true;})()`);
  const text=await evaluate(client,`document.querySelector('[aria-label="${label}"]').textContent`);
  assert.ok(text.includes(lang==='zh'?'排除数量包含在缺失数量中':'Excluded gaps are included in the missing count'));
  assert.ok(text.includes(lang==='zh'?'完整范围的差额未知':'The full-scope difference is unknown'));
  assert.equal(text.includes(lang==='zh'?'仅完整配对请求的小计':'Paired complete-request subtotals'),kind==='partial');
  assert.equal(text.includes(lang==='zh'?'额外成本':'Additional cost'),false);
  assert.ok(text.includes(`${kind==='partial'?1:0}/3`));
  for(const g of await geometry())assert.ok(g.scroll<=g.width+2,JSON.stringify(g));
  await shot(`trust-cost-gaps-${kind}-${lang}-${width}`);
  results.push({id:`trust.cost-gaps.${kind}.${lang}.${width}`,status:'passed'});
 }

 for(const [lang,theme,width,height] of [['en','light',755,600],['zh','dark',755,600],['en','dark',1200,813],['zh','light',1200,813]]){
  await open(`lang=${lang}&theme=${theme}`,width,height);
  await waitFor(client,`!!document.querySelector('.model-feed-actions')`);
  await click('.model-feed-actions button',lang==='zh'?'详情':'Details');
  await waitFor(client,`!!document.querySelector('.model-feed-details')`);
  await shot(`feed-${lang}-${theme}-${width}`);
  for(const g of await geometry()){assert.ok(g.scroll<=g.width+1,JSON.stringify(g));assert.ok(g.left>=-1 && g.right<=width+1,JSON.stringify(g));}
  await click('button',lang==='zh'?'移除本机缓存…':'Remove local copy…');
  await waitFor(client,`!!document.querySelector('[role="dialog"]')`);
  assert.equal(await evaluate(client,`document.querySelector('.model-feed-checkbox input').checked`),true);
  await shot(`feed-removal-${lang}-${theme}-${width}`);
  await click('[role="dialog"] button',lang==='zh'?'取消':'Cancel');
  assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='model_feed_remove_apply').length`),0);
  results.push({id:`feed.review.${lang}.${theme}.${width}`,status:'passed'});
 }
 await open('lang=en&stale=1');await waitFor(client,`!!document.querySelector('.model-feed-actions')`);await click('.model-feed-actions button','Details');await click('button','Remove local copy…');await waitFor(client,`!!document.querySelector('[role="dialog"]')`);await click('[role="dialog"] .btn-primary','Remove');await waitFor(client,`document.body.textContent.includes('Review the impact again')`);assert.equal(await evaluate(client,`document.querySelector('[role="dialog"]')===null`),true);results.push({id:'feed.stale-removal.repreview',status:'passed'});
 await open('lang=en&view=editor&empty=1');await waitFor(client,`!!document.querySelector('[data-trust-control="smart"]')`);assert.equal(await evaluate(client,`[...document.querySelectorAll('.trust-levels button')].some(e=>e.getAttribute('aria-pressed')==='true')`),false);assert.ok(await evaluate(client,`!!document.querySelector('.trust-control').textContent.includes('Custom 0.72')`));assert.ok(await evaluate(client,`!!document.querySelector('.trust-control').textContent.includes('No recorded evidence')`));await evaluate(client,`document.querySelector('.trust-control').scrollIntoView({block:'center'});true`);await shot('trust-custom-empty-en-755');await click('.trust-levels button','Trusted');assert.ok(await evaluate(client,`!!document.querySelector('.trust-control').textContent.includes('0.65')`));assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='preview_plan_editor').length`),0);results.push({id:'trust.custom-threshold.explicit-change',status:'passed'});
 await open('lang=en&view=editor&replay-error=1');await waitFor(client,`document.querySelector('.trust-replay')?.textContent.includes('Replay could not be read')`);await click('.trust-replay button','Retry');await waitFor(client,`document.querySelector('.trust-replay')?.textContent.includes('At 0.72')`);await click('.trust-levels button','Trusted');await waitFor(client,`document.querySelector('.trust-replay')?.textContent.includes('At 0.65')`);assert.ok(await evaluate(client,`trialTrace.some(x=>x.command==='observation_read'&&x.payload.request.intent.view==='plan_trust_replay'&&x.payload.request.intent.query.thresholds_millis[0]===650&&x.payload.request.intent.query.branch_id==='smart_saving')`));assert.ok(await evaluate(client,`document.querySelector('.trust-replay').textContent.includes('Heuristic 2')`));await evaluate(client,`document.querySelector('.trust-replay').scrollIntoView({block:'center'});true`);await shot('replay-threshold-650-en');results.push({id:'replay.custom-threshold.retry-and-exclusions',status:'passed'});
 await open('lang=zh&view=editor&theme=dark',1200,813);await waitFor(client,`!!document.querySelector('[data-trust-control="smart"]')`);await click('[role="tab"]','模型表现');await waitFor(client,`!!document.querySelector('.quality-model-row') && !!document.querySelector('.trust-suggestion')`);await shot('trust-evidence-zh-dark-1200');for(const g of await geometry())assert.ok(g.scroll<=g.width+1,JSON.stringify(g));assert.ok(await evaluate(client,`document.querySelector('.trust-suggestion').textContent.includes('41 个评分完整的阶段')`));assert.ok(await evaluate(client,`document.querySelector('.trust-scope-notes').textContent.includes('2026-10-10')`));assert.equal(await evaluate(client,`/序号|发布修订版|client_version/.test(document.querySelector('.plan-quality').textContent)`),false);results.push({id:'trust.coverage-and-version-disclosure',status:'passed'});
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
 await open('lang=en&view=timeline&stale-timeline=1');
 await waitFor(client,`document.querySelectorAll('.routing-turn').length===2`);
 await waitFor(client,`document.querySelectorAll('.routing-turn').length===3`);
 assert.equal(await evaluate(client,`document.querySelector('.routing-timeline [role=alert]')`),null);
 assert.ok(await evaluate(client,`document.querySelector('.routing-turn').textContent.includes('0.40')`));
 assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='observation_read'&&x.payload.request.intent.view==='routing_timeline')[2].payload.request.intent.query.after_watermark`),null);
 await shot('timeline-stale-cursor-recovered-en-755');
 results.push({id:'timeline.stale-cursor.rebuilds-and-appends-without-manual-refresh',status:'passed'});
 await open('lang=en&view=editor&division=1',755,600);
 await click('[role="tab"]','Model performance');await waitFor(client,`!!document.querySelector('.trust-suggestion.cyan')`);await click('.trust-suggestion button','Review division draft');
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
 await open('lang=zh&view=attention&theme=dark');await shot('attention-zh-dark-755');await click('.trust-attention .btn-primary','打开调整草稿');assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='review-trust-action').length`),1);await click('.trust-attention button','查看依据');assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='review-trust').length`),1);results.push({id:'attention.optional-evidence-review',status:'passed'});
 await open('lang=en&view=attention&empty=1');assert.ok(await evaluate(client,`document.querySelector('.trust-attention').textContent.includes('No routing adjustments are suggested')`));assert.equal(await evaluate(client,`document.querySelectorAll('.trust-attention button').length`),0);results.push({id:'attention.empty-is-normal',status:'passed'});
 await open('lang=en&view=attention&error=1');assert.ok(await evaluate(client,`document.querySelector('.trust-attention [role=alert]').textContent.includes('retry before deciding')`));assert.equal(await evaluate(client,`[...document.querySelectorAll('.trust-attention button')].find(e=>e.textContent.includes('View evidence')).disabled`),true);await click('button','Retry');assert.equal(await evaluate(client,`trialTrace.filter(x=>x.command==='retry-suggestions').length`),1);results.push({id:'attention.failed-read-is-not-empty',status:'passed'});
 }
 for(const [lang,theme,lost] of [['en','light',false],['zh','dark',true]]) {
  await open(`lang=${lang}&theme=${theme}&view=quickstart${lost?'&lost=1&saved-only=1':''}`,755,600);
  await click('.new-models button',lang==='zh'?'试用':'Try');
  await waitFor(client,`!!document.querySelector('.trial-page .trial-agent-choice')`);
  assert.equal(await evaluate(client,`document.querySelector('.trial-steps [aria-current=step]').textContent.includes(${JSON.stringify(lang==='zh'?'选择模型':'Choose models')})`),true);
  assert.equal(await evaluate(client,`[...document.querySelectorAll('.trial-presets input:checked')].length`),1);
  assert.equal(await evaluate(client,`document.querySelector('.trial-choice-list .trial-choice[aria-pressed=true]')`),null);
  assert.equal(await evaluate(client,`document.querySelector('.trial-footer .btn-primary').disabled`),true);
  await evaluate(client,`(()=>{const el=[...document.querySelectorAll('.trial-choice-list .trial-choice')].find(e=>e.textContent.includes(primaryModel.display_name));if(!el)throw Error('Missing primary choice');el.click();return true;})()`);
  await waitFor(client,`!!document.querySelector('.trial-choice.selected[aria-expanded=false]')`);
  await shot(`quickstart-setup-${lang}-${theme}`);
  await evaluate(client,`document.querySelector('.trial-footer').scrollIntoView({block:'end'});true`);
  assert.ok(await evaluate(client,`(()=>{const r=document.querySelector('.trial-footer .btn-primary').getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight+1;})()`));
  await shot(`quickstart-actions-${lang}-${theme}`);
  await click('.trial-footer button',lang==='zh'?'检查改动':'Review changes');
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
  assert.ok(await evaluate(client,`document.querySelector('.trial-steps [aria-current=step]').textContent.includes(${JSON.stringify(lang==='zh'?'开始使用':'Start using it')})`));
  assert.equal(await evaluate(client,`trialTrace.find(x=>x.command==='preview_agent_settings').payload.input.spec.model.settings.preset_mappings.sonnet.kind`),'preserve_native');
  assert.equal(await evaluate(client,`trialTrace.find(x=>x.command==='preview_agent_settings').payload.input.spec.collaboration.intent`),'keep');
  for(const g of await evaluate(client,`[...document.querySelectorAll('.trial-wizard,.trial-mapping')].map(e=>({width:e.clientWidth,scroll:e.scrollWidth,left:e.getBoundingClientRect().left,right:e.getBoundingClientRect().right}))`)) {assert.ok(g.scroll<=g.width+1,JSON.stringify(g));assert.ok(g.left>=-1&&g.right<=756,JSON.stringify(g));}
  await shot(`quickstart-complete-${lang}-${theme}`);
  results.push({id:`quickstart.${lang}.${lost?'lost-publication-response':'first-mapping'}.no-duplicate`,status:'passed'});
 }
 await writeFile(path.join(output,'report.json'),JSON.stringify({proof:'production components with synthetic IPC; no native or provider claim',results},null,2));console.log(JSON.stringify({passed:results.length,output}));
} catch(error) {await shot('failure');await writeFile(path.join(output,'failure.json'),JSON.stringify(await evaluate(client,`({text:document.body.innerText,trace:trialTrace})`),null,2));throw error;} finally {clearInterval(keepAlive);client.close();}
