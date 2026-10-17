import { test } from 'node:test';
import assert from 'node:assert/strict';
import { currentPrimary, trialMappingValues, initialTrialMapping } from '../src/features/trial/model.ts';
const native = { agent_id:'agent_claude_default', context_id:'claude/context', status_error:null, settings:{state:'not_configured'} };
const configured = { ...native, settings:{state:'configured',current_selection:{mode:'claude_launcher',surfaces:['claude_cli'],fixed_models:[],preset_mappings:{opus:{kind:'plan',plan_id:'primary'},sonnet:{kind:'plan',plan_id:'separate'},haiku:{kind:'preserve_native'}}}} };
test('first Claude trial maps only the explicitly selected Opus preset',()=>{
 const values=trialMappingValues(native,'trial',{agentId:native.agent_id,presets:['opus']});
 assert.deepEqual(values.claudePresets,{opus:{kind:'plan',plan_id:'trial'},sonnet:{kind:'preserve_native'},haiku:{kind:'preserve_native'}});
});
test('trial preserves a separately configured Sonnet and all fixed-model bindings',()=>{
 const values=trialMappingValues(configured,'trial',{agentId:configured.agent_id,presets:['opus']});
 assert.deepEqual(values.claudePresets.sonnet,configured.settings.current_selection.preset_mappings.sonnet);
 assert.deepEqual(configured.settings.current_selection.preset_mappings.opus,{kind:'plan',plan_id:'primary'});
 assert.throws(()=>trialMappingValues({...configured,status_error:'unavailable'},'trial',{agentId:configured.agent_id,presets:['opus']}));
});
test('Primary defaults only from a unique enabled fixed-plan resolution, never a native alias',()=>{
 const plan={agent_plan_id:'primary',head:{status:'enabled'},desired:{mode:'fixed_model',strategy:{candidates:[{binding_id:'b',reasoning:{kind:'profile',profile:'high'}}]}}};
 assert.equal(currentPrimary(native,[plan]),undefined);
 assert.deepEqual(currentPrimary(configured,[plan]),plan.desired.strategy.candidates[0]);
 assert.equal(currentPrimary(configured,[{...plan,head:{status:'disabled'}}]),undefined);
 assert.equal(currentPrimary(configured,[{...plan,desired:{...plan.desired,strategy:{candidates:[{binding_id:'a'},{binding_id:'b'}]}}}]),undefined);
 assert.deepEqual(initialTrialMapping({agents:[native],plans:{plans:[]}}),{agentId:'agent_claude_default',presets:['opus']});
});
