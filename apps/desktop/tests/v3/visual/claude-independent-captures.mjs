import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { evaluate, navigate, waitFor } from './cdp-client.mjs';

// Real production forms and styles over synthetic host replies; no native/model claim.
export async function runClaudeIndependentCaptures(client, fixtureUrl, outputRoot) {
  const checks = [];
  const read = expression => evaluate(client, expression);
  const click = (selector, label) => read(`(() => { const e = [...document.querySelectorAll(${JSON.stringify(selector)})].find(e => ${label === undefined ? 'true' : `e.textContent.trim() === ${JSON.stringify(label)}`}); if (!e || e.disabled) throw Error('Control unavailable: ' + ${JSON.stringify(selector)}); e.scrollIntoView({block:'center'}); e.click(); return true; })()`);
  const choose = (selector, value) => read(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) throw Error('Missing select'); Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(e, ${JSON.stringify(value)}); e.dispatchEvent(new Event('change',{bubbles:true})); return true; })()`);
  for (const language of ['zh', 'en']) for (const width of [755, 1200]) for (const theme of ['light', 'dark']) {
    const viewport = { width, height: width === 755 ? 600 : 813 };
    const t = (zh, en) => language === 'zh' ? zh : en;
    const prefix = `claude-independent-${language}-${width}-${theme}`;
    const shots = [];
    async function capture(state, selector) {
      await read(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'center'}); true`);
      const bounds = await read(`(() => {const nodes=[...document.querySelectorAll(${JSON.stringify(selector + ', [data-claude-activation-choice] .field-label, [role=dialog] .modal-footer')})].filter(e=>e.getClientRects().length); return nodes.map(e=>{const r=e.getBoundingClientRect();return {width:e.clientWidth,scroll:e.scrollWidth,left:r.left,right:r.right,top:r.top,bottom:r.bottom};});})()`);
      assert.ok(bounds.length > 0);
      for (const r of bounds) { assert.ok(r.scroll <= r.width + 1, JSON.stringify(r)); assert.ok(r.left >= -1 && r.right <= width + 1, JSON.stringify(r)); }
      const file = `${prefix}-${state}.png`;
      const shot = await client.send('Page.captureScreenshot', { format: 'png' });
      await writeFile(path.join(outputRoot, file), Buffer.from(shot.data, 'base64')); shots.push(file);
    }
    await navigate(client, fixtureUrl(`agent-trust.html?language=${language}&theme=${theme}`), viewport);
    await waitFor(client, 'Boolean(window.agentTrust)');
    await read(`(() => { const c=agentTrust; c.reset(); const a=c.agents.agents.find(a=>a.agent_id==='agent_claude_default'); a.settings={...a.settings,state:'not_configured',current_selection:null,restore_point_ref:null};
      c.handlers.preview_agent_settings=({input:{spec}})=>{a.settings={...a.settings,state:'configured',activation_mode:spec.activation_preference==='managed_launch'?'managed_launch':undefined,current_selection:spec.model.settings,restore_point_ref:'restore/independent',operation_id:'operation/agent-settings-fixture',operation_state:'succeeded'};return {preview:{applicable:true,blockers:[]},mutation:{state:'applied',operation:{operation_id:'operation/agent-settings-fixture',state:'succeeded',sequence:1,cancellable:false}}};};
      c.handlers.preview_agent_previous_routing=()=>({status:'available',spec:{context_id:a.context_id,activation_preference:'managed_launch',model:{intent:'configure',settings:a.settings.current_selection}},restored_from:{opus:'operation/earlier'}});return true; })()`);
    await waitFor(client, '!!document.querySelectorAll(".native-list .list-row").length');
    await read(`[...document.querySelectorAll('.native-list .list-row')].find(e=>e.textContent.includes('Claude')).click();true`);
    await click('[data-agent-facet="model"]');
    await waitFor(client, '!!document.querySelector("[data-claude-activation-choice] select")');
    assert.equal(await read('document.querySelector("[data-claude-activation-choice] select").value'), 'automatic');
    await choose('[data-claude-activation-choice] select', 'managed_launch');
    await capture('choice', '[data-claude-activation-choice]');
    await click('[role=dialog] button', t('取消', 'Cancel'));
    await waitFor(client, '!document.querySelector("[role=dialog]")');
    assert.equal(await read('agentTrust.commands.filter(c=>c.command==="preview_agent_settings").length'), 0);
    await click('[data-agent-facet="model"]');
    await waitFor(client, '!!document.querySelector("[data-claude-activation-choice] select")');
    assert.equal(await read('document.querySelector("[data-claude-activation-choice] select").value'), 'automatic', 'Cancel must discard the activation choice');
    await choose('[data-claude-activation-choice] select', 'managed_launch');
    await click('button[form="hr-agent-settings"][type="submit"]');
    await waitFor(client, '!!document.querySelector("[data-claude-launch-command]")');
    assert.equal(await read('agentTrust.commands.find(c=>c.command==="preview_agent_settings").payload.input.spec.activation_preference'), 'managed_launch');
    assert.equal(await read('document.querySelector("[data-claude-launch-command] code").textContent'), 'hiroute agent launch --agent claude-code --');
    await capture('connected', '[data-claude-launch-command]');
    await read(`(() => {const s=[...document.querySelectorAll('summary')].find(e=>e.textContent.trim()===${JSON.stringify(t('回到之前的路由', 'Return to previous routing'))});s.click();return true;})()`);
    await click('button', t('查看之前的配置', 'Find previous mappings'));
    await waitFor(client, `Array.from(document.querySelectorAll('button')).some(e=>e.textContent===${JSON.stringify(t('审阅路由修改', 'Review routing changes'))})`);
    await click('button', t('审阅路由修改', 'Review routing changes'));
    await waitFor(client, '!!document.querySelector("[data-claude-activation-choice] select")');
    assert.equal(await read('document.querySelector("[data-claude-activation-choice] select").value'), 'managed_launch', 'Previous routing lost the independent target');
    await click('[role=dialog] button', t('取消', 'Cancel'));

    // Exercise the complete quick start: choice before publication, confirmed mapping,
    // unchanged other presets, and the launch command only after fresh status confirms it.
    await navigate(client, fixtureUrl(`new-model-trial.html?lang=${language}&theme=${theme}&view=quickstart`), viewport);
    await waitFor(client, '!!document.querySelector(".new-models button")');
    await click('.new-models button', t('试用', 'Try'));
    await waitFor(client, '!!document.querySelector(".trial-agent-choice")');
    await choose('[data-claude-activation-choice] select', 'managed_launch');
    await capture('quickstart-choice', '[data-claude-activation-choice]');
    await read(`(() => { const e=[...document.querySelectorAll('.trial-choice-list .trial-choice')].find(e=>e.textContent.includes(primaryModel.display_name)); if(!e)throw Error('Missing Primary');e.click();return true; })()`);
    await waitFor(client, '!!document.querySelector(".trial-footer .btn-primary:not(:disabled)")');
    await click('.trial-footer button', t('检查改动', 'Review changes'));
    await waitFor(client, '!!document.querySelector(".trial-review > .actions .btn-primary:not(:disabled)")');
    await click('.trial-review button', t('保存并发布路由', 'Save and publish route'));
    await waitFor(client, '!!document.querySelector(".trial-mapping .trial-agent-choice")');
    assert.equal(await read('document.querySelector("[data-claude-activation-choice] select").value'), 'managed_launch');
    assert.equal(await read('document.querySelector("[data-claude-launch-command]")'), null);
    await click('.trial-mapping button', t('检查并映射 Agent', 'Review and map Agent'));
    await waitFor(client, '!!document.querySelector("[data-claude-launch-command]")');
    const spec = await read('trialTrace.find(c=>c.command==="preview_agent_settings").payload.input.spec');
    assert.equal(spec.activation_preference, 'managed_launch'); assert.equal(spec.model.settings.preset_mappings.sonnet.kind, 'preserve_native'); assert.equal(spec.collaboration.intent, 'keep');
    await capture('quickstart-confirmed', '[data-claude-launch-command]');
    const id = `claude.independent.shared-flow.${language}.${width}.${theme}`;
    checks.push({ id, name: id, state: 'green', language, viewport, theme, textScale: 1, screenshots: shots });
  }
  assert.equal(checks.length, 8, 'Required language/window/theme combinations were not executed');
  return checks;
}
