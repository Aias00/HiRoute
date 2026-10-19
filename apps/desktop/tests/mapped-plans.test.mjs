import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mappedPlanList } from '../src/features/agents/mapped-plans.ts';
test('complete Codex mapping list keeps all ten Plans and annotates its sole default', () => {
  const ids=Array.from({length:10},(_,i)=>`plan/${i}`);
  const rows=mappedPlanList({mode:'codex_default',allowed_plan_ids:ids,default_selection:{kind:'plan',plan_id:ids[0]}});
  assert.equal(rows.length,10);assert.deepEqual(rows[0],{planId:ids[0],slots:['default']});
  assert.equal(rows.slice(1).filter(row=>row.slots.length===0).length,9);
});
test('Claude slots are deduplicated in preset order and native models are absent', () => {
  assert.deepEqual(mappedPlanList({mode:'claude_launcher',preset_mappings:{opus:{kind:'plan',plan_id:'a'},sonnet:{kind:'plan',plan_id:'b'},haiku:{kind:'plan',plan_id:'b'}}}),[{planId:'a',slots:['opus']},{planId:'b',slots:['sonnet','haiku']}]);
  assert.deepEqual(mappedPlanList({mode:'claude_launcher',preset_mappings:{opus:{kind:'preserve_native'},sonnet:{kind:'plan',plan_id:'b'},haiku:{kind:'preserve_native'}}}),[{planId:'b',slots:['sonnet']}]);
  assert.deepEqual(mappedPlanList(null),[]);
});
