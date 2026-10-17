import { useState } from 'react';
import { TrialQuickStart } from '../../../src/features/trial/TrialQuickStart';
import { NewModels } from '../../../src/features/trial/NewModels';
import { readyDesktop, readyAgents, readyManagement, dailyPlan, mockProductInvoke } from './product-fixtures';
import type { DesktopOperation, DesktopSnapshot } from '../../../src/product/home-projections';
const desktop = structuredClone(readyDesktop); desktop.catalog = { plans: [], drafts: [] }; desktop.pending = null;
const agents = structuredClone(readyAgents); agents.agents = [{...agents.agents.find(a=>a.agent_id==='agent_claude_default')!,context_id:'agent-context/claude/default',settings:{state:'not_configured',restore_point_ref:null,model_verified:false}}];agents.plans.plans=[];
const decision = {id:'decision/trial',revision:1,name:'Saved decision model',connection:{kind:'custom',endpoint:'https://decision.example.test',timeout_ms:10000}};
let observed:DesktopOperation|null=null;
let publishLost=false;
const catalog = readyManagement.sources.flatMap(source=>source.models.map(model=>({model_configuration_id:model.catalog_configuration_id??model.upstream_model_id,display_name:model.display_name,origin:{kind:'embedded',client_version:'0.3.1'},identity_stability:'rolling-alias',availability:{kind:'executable'},binding_ids:[model.binding_id]})));
export const trialModel = catalog.find(m=>m.binding_ids.includes('binding/bailian/qwen-coder'))!;
export const primaryModel = catalog.find(m=>m.binding_ids.includes('binding/codex/gpt-5.6-sol'))!;
Object.assign(window,{trialModel,primaryModel});
export async function quickStartInvoke(command:string,payload:any,params:URLSearchParams) {
 if(command==='model_availability') return {featured:[],models:catalog};
 if(command==='desktop_snapshot') return structuredClone(desktop);
 if(command==='agent_snapshot') return structuredClone(agents);
 if(command==='decision_services') return {services:[decision]};
 if(command==='observe_operation') return observed;
 if(command==='preview_plan_editor') {
  const i=payload.input;
  observed={operation_id:`operation/${i.action}`,state:'succeeded',sequence:1,cancellable:false,safe_error_code:null};
  desktop.pending={plan_id:i.plan_id??i.draft_id,operation_id:observed.operation_id,idempotency_key:'fixture'};
  if(i.action==='save_draft') desktop.catalog.drafts=[{draft_id:i.draft_id,revision:1,editor:structuredClone(i.editor)}];
  if(i.action==='publish') {
   const plan={...structuredClone(dailyPlan),agent_plan_id:'plan/trial',model_alias:i.editor.custom_alias,head:{head_revision:1,status:'enabled'},agent_plan_revision:1,desired:{...structuredClone(dailyPlan.desired),display_name:i.editor.display_name,purpose:i.editor.purpose,mode:i.editor.mode,strategy:{mode:'smart_saving',...i.editor.smart}}};
   desktop.catalog.plans=[plan];desktop.catalog.drafts=[];agents.plans.plans=[plan];
   if(params.has('lost')&&!publishLost){publishLost=true;throw Error('TRANSPORT_LOST_AFTER_COMMIT');}
  }
  return {state:'succeeded',operation:observed};
 }
 if(command==='preview_agent_settings') {
  const agent=agents.agents[0];
  agent.settings={state:'configured',restore_point_ref:'restore/test',model_verified:false,operation_id:'operation/map',operation_state:'succeeded',current_selection:payload.input.spec.model.settings};
  return {preview:{applicable:true,blockers:[]},mutation:{state:'applied',operation:{operation_id:'operation/map',state:'succeeded',sequence:1,cancellable:false}}};
 }
 if(command==='observation_read') return {rows:[],branches:[],sessions:[],announcements:[]};
 return mockProductInvoke(command,payload,'ready');
}
export function TrialFixture({language}:{language:'zh'|'en'}) {
 const [snapshot,setSnapshot]=useState<DesktopSnapshot>(structuredClone(desktop));
 const [operation,setOperation]=useState<DesktopOperation|null>(null);
 const [open,setOpen]=useState(false);
 return <><NewModels language={language} active plans={snapshot.catalog.plans} refreshVersion={0} onTry={()=>setOpen(true)} />{open&&<TrialQuickStart initialModelId={trialModel.model_configuration_id} language={language} mutable snapshot={snapshot} operation={operation} onOperation={setOperation} onRefresh={async()=>{setSnapshot(structuredClone(desktop));setOperation(observed);}} onClose={()=>setOpen(false)} onOpenAgent={()=>{}} />}</>;
}
