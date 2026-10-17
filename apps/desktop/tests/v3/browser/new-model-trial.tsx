import { TrialFixture, quickStartInvoke } from './trial-quickstart-fixture';
import { TrustAttention } from '../../../src/features/trust/TrustAttention';
import { createRoot } from 'react-dom/client';
import { mockIPC } from '@tauri-apps/api/mocks';
import { Sessions } from '../../../src/features/Sessions';
import { Models } from '../../../src/features/models/Models';
import { PlanEditor } from '../../../src/plan-editor';
import { PresentationRoot, ProductPage } from '../../../src/ui';
import { readyManagement, dailyPlan, mockProductInvoke } from './product-fixtures';
import type { ModelFeedStatus } from '../../../src/features/model-feed/types';
import type { TrustReport } from '../../../src/features/trust/types';
import '../../../src/occami/styles.css';
const params = new URLSearchParams(location.search);
const language = params.get('lang') === 'zh' ? 'zh' : 'en';
const trace: { command: string; payload: any }[] = [];
Object.assign(window, { trialTrace: trace });
const feed: ModelFeedStatus = { retrieval_enabled:true, held:{sequence:12, issued_at:'2026-10-18T00:00:00Z', retrieved_at:1792281600000}, highest_accepted_sequence:12, last_attempt:{at:1792281600000,result:'ok',reason:null}, verification:{kind:'verified'}, counts:{applied:4,ignored_embedded:2,newer_client:1,unresolved:0}, next_automatic_attempt_at:null, disclosure:'Model information is retrieved automatically from hiroute.ai without account or usage identifiers. You can turn off automatic retrieval.', disclosure_acknowledged:false };
const counts = { executed:58,complete:41,meets_floor:37,partial:4,awaiting_assessment:1,unscored:12,sessions:9 };
const report: TrustReport = { plan_id:dailyPlan.agent_plan_id, active_revision:8, revision:'active',from_ms:0,to_ms:Date.now(), rows:[{branch_id:'smart_saving',group:'regular',model_configuration_id:'model.trial',reasoning_profile_id:'high',attribution:'single',executor:null,identity_stability:'rolling-alias',counts,competence_rate:37/41,scoring_coverage:41/58,execution_share:0.8,protections:[],availability_relays:0,duration_p50_ms:1500,duration_p90_ms:3500,output_tokens_per_second:50}], branches:[{branch_id:'smart_saving',threshold_millis:720,full_history:counts,upward_cohort:counts,upward_from_ms:0,suggestion:{kind:'step_up',threshold_millis:650},replay:{included:58,excluded_by_reason:{heuristic:2},recorded_share_at_current:0.8,estimate_only:true,thresholds:[{threshold_millis:650,regular:49,primary:9,protected_as_primary:3,regular_share:49/58,newly_regular:7}]}}],sessions:[],session_total:9,executors:[{executor:'deepseek_harness',task_outcomes:{submitted:3,succeeded:2,failed:1}}],announcements:[{model_configuration_id:'model.trial',included_on:'2026-10-10',before:31,announcement_day:2,after:25,executed_version_verified:false}],cost:{kind:'unavailable',reason:'comparison basis not available'} };
const timelineRequests = (params.has('stale-timeline') ? [1,2,3] : [1,2]).map(i => ({request_id:`request-${i}`,started_at_ms:Date.now()-10000+i*1000,duration_ms:1500,native_model:i===1?'primary-model':'trial-model',input_tokens:1000,output_tokens:200,cache_read_tokens:990,cache_read_share:0.99,costs:[{model_configuration_id:i===1?'Primary':'Trial',currency:'USD',valuation_kind:'usage_estimate',known_micros:1250,unknown_contributions:i===1?0:1}],outcome:'completed'}));
let timelinePolls=0;
const timelineTurn = (i:number,updated=false) => ({turn_id:`request-${i}`,agent_turn_id:`turn-${i}`,segment_id:`stage-${i}`,started_at_ms:timelineRequests[i-1].started_at_ms,finished_at_ms:timelineRequests[i-1].started_at_ms+1500,boundary:i===1?'new_user_message':'context_rebuild',plan_id:dailyPlan.agent_plan_id,plan_revision:8,branch_id:'smart_saving',model_configuration_id:i===1?'Primary model with a long readable name':'Trial model',reasoning_profile_id:'high',group:i===1?'primary':'regular',selection:{simple_probability:i===1?0.62:0.94,simple_threshold_millis:900,selection_reason:'degree',competence_trigger:null},tool_continuations:1,relays:[],assessment_state:updated?'scored':'pending',assessment:updated?{score:0.4,partial:false,target_from_ordinal:1,target_through_ordinal:1}:null,competence_floor_millis:500,protected_turn_ids:[],status:'completed',requests:[timelineRequests[i-1]],cost_by_model:timelineRequests[i-1].costs,evidence_partial:false,content_digest:`digest-${i}-${updated}`});
let failed = false;
mockIPC(async (command, payload: any) => {
  trace.push({command,payload});
  if(params.get('view')==='quickstart') return quickStartInvoke(command,payload,params);
  if (command === 'worker_executor_availability') return {executors:['deepseek_harness','codex_cli','claude_code','pi','qoder_cli'].map(harness=>({harness,state:harness==='deepseek_harness'?'ready':'unavailable'}))};
  if (command === 'model_feed_status') { if (params.has('error') && !failed) {failed=true;throw new Error('DAEMON_UNAVAILABLE');} return structuredClone(feed); }
  if (command === 'model_feed_refresh') { feed.last_attempt={at:Date.now(),result:'signature',reason:'invalid_signature'}; return structuredClone(feed); }
  if (command === 'model_feed_settings_apply') { if (payload.input.retrieval_enabled !== undefined) feed.retrieval_enabled=payload.input.retrieval_enabled; if (payload.input.acknowledge_disclosure) feed.disclosure_acknowledged=true; return structuredClone(feed); }
  if (command === 'model_feed_remove_preview') return { sources:[{source_id:'source/feed',display_name:'Team experimental model connection with a deliberately long readable name',model_configuration_ids:['model.trial'],published_revisions:[]}],published_revisions:[{plan_id:'plan/frozen',content_revision:3}],retained_sequence:12,retrieval_enabled:feed.retrieval_enabled,digest:'sha256:preview' };
  if (command === 'model_feed_remove_apply') { if (params.has('stale')) throw {message_key:'application.error.change_preview_stale'}; feed.held=null; feed.verification={kind:'none'}; if (payload.input.also_disable_retrieval) feed.retrieval_enabled=false; return structuredClone(feed); }
  if (command === 'model_availability') return {featured:[],models:[]};
  if (command === 'observation_read' && payload.request.intent.view === 'plan_trust_replay') {
    const threshold=payload.request.intent.query.thresholds_millis[0];
    if(params.has('replay-error') && !failed) {failed=true;throw Error('OBSERVATION_SCOPE_TOO_LARGE');}
    return {included:params.has('empty')?0:58,excluded_by_reason:{heuristic:2},recorded_share_at_current:params.has('empty')?null:0.8,estimate_only:true,thresholds:[{threshold_millis:threshold,regular:params.has('empty')?0:49,primary:params.has('empty')?0:9,protected_as_primary:params.has('empty')?0:3,regular_share:params.has('empty')?null:49/58,primary_share:params.has('empty')?null:9/58,newly_regular:params.has('empty')?0:7}]};
  }
  if (command === 'observation_read' && payload.request.intent.view === 'plan_trust') {
    const value=structuredClone(report); if(params.has('division')) value.branches[0].suggestion={kind:'explore_division'}; if (params.has('empty')) {value.rows=[]; value.branches[0].full_history={executed:0,complete:0,meets_floor:0,partial:0,awaiting_assessment:0,unscored:0,sessions:0};}
    if (payload.request.intent.query.group_by === 'executor') value.rows.forEach(row => row.executor='deepseek_harness');
    return value;
  }
  if (command === 'observation_read' && params.get('view') === 'timeline') {
    const {view,query}=payload.request.intent;
    if(view==='routing_timeline') {
      timelinePolls++;
      if(params.has('stale-timeline') && query.after_watermark==='watermark-1') throw {message_key:'application.error.change_preview_stale'};
      const turns=params.has('stale-timeline') && timelinePolls>1 && query.after_watermark===null ? [timelineTurn(1,true),timelineTurn(2),timelineTurn(3)] : timelinePolls===1?[timelineTurn(1),timelineTurn(2)]:timelinePolls===2?[timelineTurn(1,true)]:[];
      return {turns,watermark:`watermark-${timelinePolls}`,complete:true};
    }
    if(view==='timeline') return {requests:timelineRequests.map(r=>({...r,native_turn_id:r.request_id,within_request_fallback:false,final_native_model:r.native_model,between_turn_model_change:false,previous_native_turn_id:null,previous_turn_model:null,turn_final_native_model:r.native_model})),next_cursor:null};
    if(view==='sessions') return {sessions:[{session_id:'timeline-session',agent_id:'claude-code',request_count:2,fallback_request_count:0,last_request_at_ms:Date.now(),correlation_kind:'confirmed',content_completeness:'complete'}],next_cursor:null};
    if(view==='status') return {running:true,index_running:true,error_count:0};
    if(view==='catalog') return {contents:[],next_cursor:null};
    if(view==='ancestry') return {roots:[],gap:null};
    if(view==='facts') return {facts:[],projection_partial:false,next_cursor:null};
  }
  if (command === 'observation_read') return {samples:[],summary:{models:[],scored_stage_count:0,unrated_stage_count:0,session_count:0,available_revisions:[8]},next_cursor:null};
  const value = await mockProductInvoke(command,payload,'ready');
  if(command==='plan_editor_options' && params.has('division')) for(const c of (value as any).candidates) if(c.binding_id==='binding/bailian/qwen-coder') {c.model_configuration_id='deepseek-v4.1-flash';c.display_name='DeepSeek trial';}
  return value;
});
const plan = structuredClone(dailyPlan);
plan.desired.delegation_enabled=false;
plan.desired.strategy.judgment!.degree.simple_threshold_millis=720;
plan.desired.strategy.classifier={kind:'decision_service',service:{id:'decision/test',revision:1,name:'Decision model',connection:{kind:'custom',endpoint:'https://decision.example.test',timeout_ms:1000,auth_header:{kind:'none'}}}};
createRoot(document.getElementById('root')!).render(<PresentationRoot language={language} theme={params.get('theme') === 'dark' ? 'dark' : 'light'} textScale={1}>
  {params.get('view') === 'quickstart' ? <TrialFixture language={language} /> : params.get('view') === 'attention' ? <ProductPage title="Home"><TrustAttention language={language} names={{[dailyPlan.agent_plan_id]:'My daily route'}} read={{status:params.has('error')?'error':'ready',plans:params.has('empty')?[]:[report],error:'OBSERVATION_UNAVAILABLE',refresh:()=>trace.push({command:'retry-suggestions',payload:{}})}} onAct={(planId,branchId)=>trace.push({command:'review-trust-action',payload:{planId,branchId}})} onOpen={planId=>trace.push({command:'review-trust',payload:{planId}})} /></ProductPage> : params.get('view') === 'timeline' ? <Sessions language={language} initialSession="timeline-session" /> : params.get('view') === 'editor' ? <ProductPage title="Routing"><PlanEditor plan={plan} language={language} onCreateDraft={async editor=>{ Object.assign(window,{divisionDraft:editor}); return false; }} onDone={async()=>{}} onClose={()=>{}} /></ProductPage> : <ProductPage title="Models" flush className="hr-models-page"><Models language={language} snapshot={readyManagement} mutable onRefresh={async()=>{}} onPrepareKeyInput={async()=>{throw Error('outside scenario')}} onPreview={async()=>{throw Error('outside scenario')}} onApply={async()=>''} onDiscardProtectedInputs={()=>{}} /></ProductPage>}
</PresentationRoot>);
