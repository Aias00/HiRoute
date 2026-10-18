import { branchRouting } from '../../../src/features/decision-services/types.ts';
import { defaultJudgment } from '../../../src/features/decision-services/types.ts';
import { nativeProtocol, stockAssessment, contentDigest } from '../../../src/features/decision-services/native-protocol.ts';
import { scenario } from './scenario-selection.mjs';

export function cProtocolScenarios({ fresh, routing, click, until, all, text, calls, setInput, control }) {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const service = { id:'decision/ui-fixture',revision:7,name:'Saved decision fixture',connection:{kind:'system_one',provider:'compatible',model:'synthetic-model',endpoint:'https://decision.example/systemone',timeout_ms:3000,auth_header:{name:'Authorization',value_secret_ref:'decision/ui-fixture/r7'}} };
  async function open(language, { enabled=false, branches=false, custom=false }={}) {
    await fresh(() => {
      control().handlers.decision_services=()=>({services:[service]});
      control().handlers.desktop_snapshot=payload=>{
        const snapshot=structuredClone(control().fixtureResponse('desktop_snapshot',payload));
        const plan=snapshot.catalog.plans[0],strategy=plan.desired.strategy;
        strategy.classifier={kind:'decision_service',service};
        if(enabled){strategy.native_protocol=nativeProtocol();strategy.judgment=stockAssessment(strategy.judgment,strategy.native_protocol);}
        if(custom){strategy.judgment.competence.standard='custom';strategy.judgment.competence.instructions='  Keep this exact text.\nSecond line.  ';strategy.judgment.competence.criteria[1]+='  ';}
        plan.desired.delegation_enabled=false;
        if(branches){
          const route=branchRouting(service);route.judgment=structuredClone(strategy.judgment);
          if(enabled)route.native_protocol=nativeProtocol();
          route.branches.forEach((b,i)=>{b.name='Category '+i;b.condition='Work '+i;b.candidates=structuredClone(strategy.economy);b.primary_candidates=structuredClone(strategy.primary);b.judgment=i===0?structuredClone(strategy.judgment):null;});
          plan.desired.mode='custom_branches';plan.desired.strategy={mode:'branches',routing:route};
        }
        return snapshot;
      };
    });
    localStorage.setItem('hiroute.language',language);control().remount();
    if(language==='zh')await routing();else {await click('Smart routing');await until(()=>all('.master-list .list-row').length,'routes');all('.master-list .list-row')[0].click();}
    await until(()=>document.querySelector('.native-protocol-editor input[type="checkbox"]'),'capability toggle');
    await until(()=>calls('plan_editor_options').some(c=>c.payload.input.editor?.smart.classifier.kind==='decision_service'),'loaded route options');
  }
  const toggle=()=>document.querySelector('.native-protocol-editor input[type="checkbox"]');
  function assertCopy(){
    const visible=text();
    assert(!/hiroute\.system-one|sha256:|Stock wording|Stock C|C decisions|Question 3|第三问|恢复版本|Confirm assessments|审阅/.test(visible),'Implementation or review wording remains visible');
    assert(!document.querySelector('[data-assessment-transition]'),'Extra transition review remains');
  }
  function geometry(){for(const e of all('.native-protocol-editor .field-label,.native-prompt,.native-protocol-editor button'))assert(e.scrollWidth<=e.clientWidth+1,'Capability content overflows');}
  async function publish(t){
    const before=calls('preview_plan_editor').length;
    await click(t('发布更改','Publish changes'));
    await until(()=>calls('preview_plan_editor').length>before,'ordinary publication dispatch');
    const input=calls('preview_plan_editor').at(-1).payload.input;
    assert(!('assessment_choices' in input)&&!JSON.stringify(input.editor).includes('assessment_review'),'Review metadata entered publication');
    assert(!calls('plan_editor_transition_preview').length,'Extra Preview operation remains');
    return input;
  }
  return [scenario('desktop.routing.capability-toggle',['routing-editor','decision-services','c-protocol'],'Defaults silently follow the capability through ordinary publication',async()=>{
    for(const language of ['zh','en']){
      const t=(zh,en)=>language==='zh'?zh:en;await open(language);assert(!toggle().checked,'Released route enabled itself');
      toggle().click();await until(()=>toggle().checked,'enabled');
      assertCopy();geometry();
      const on=await publish(t);assert(Object.keys(on.editor.smart.native_protocol).length===2,'Editor requires fixed bundle');
      assert(on.editor.smart.judgment.competence.standard==='default','Default intent lost');
      assert(on.editor.smart.judgment.competence.floor_millis===500&&on.editor.smart.judgment.degree.simple_threshold_millis===800,'Threshold changed');
      await until(()=>!all('.plan-editor fieldset')[0]?.disabled,'publication completed');
      toggle().click();await until(()=>!toggle().checked,'disabled');
      assert(document.querySelector('[data-decision-field="smart-competence-instructions"]').value===defaultJudgment.competence.instructions,'Default failed to follow toggle off');
      assertCopy();geometry();assert(!calls('test_classifier_decision').length,'Toggle/publication dispatched a model request');
    }
  }),scenario('desktop.routing.custom-assessment',['routing-editor','decision-services','c-protocol'],'Custom text survives both toggle directions and reset restores active defaults',async()=>{
    for(const language of ['zh','en'])for(const branches of [false,true]){
      const t=(zh,en)=>language==='zh'?zh:en;await open(language,{custom:true,branches});
      const selector=`[data-decision-field="${branches?'branch-0':'smart'}-competence-instructions"]`;
      const original=document.querySelector(selector).value;
      toggle().click();await until(()=>toggle().checked,'enabled');assert(document.querySelector(selector).value===original,'Custom changed on enable');
      toggle().click();await until(()=>!toggle().checked,'disabled');assert(document.querySelector(selector).value===original,'Custom changed on disable');
      toggle().click();await until(()=>toggle().checked,'enabled again');
      const field=document.querySelector(selector),parents=[];for(let p=field.parentElement;p;p=p.parentElement)if(p.tagName==='DETAILS')parents.unshift(p);for(const p of parents)if(!p.open)p.querySelector('summary').click();
      const disclosure=field.closest('details');const reset=[...disclosure.querySelectorAll('button')].find(b=>b.textContent===t('恢复默认','Reset to default'));assert(reset,'Reset missing');reset.click();
      await until(()=>document.querySelector(selector).value!==original,'reset active default');assertCopy();geometry();
      const input=await publish(t);const j=branches?input.editor.branch_routing.branches[0].judgment:input.editor.smart.judgment;
      assert(j.competence.standard==='default','Reset did not record intent');assert(!calls('test_classifier_decision').length,'Edit implicitly probed');
    }
  }),scenario('desktop.routing.applicability-test',['routing-editor','decision-services','c-protocol'],'The route test binds actual content and discards evidence after changes',async()=>{
    for(const language of ['zh','en']){
      const t=(zh,en)=>language==='zh'?zh:en;await open(language,{enabled:true});
      control().handlers.test_classifier_decision=async({input})=>{
        const request=input.request;assert(request.schema==='hiroute.historical-applicability-test/v1'&&request.service_revision===7,'Wrong purpose or revision');
        return {schema:'hiroute.historical-applicability-test-result/v1',state:'verified',service_id:service.id,service_revision:7,model:service.connection.model,endpoint_digest:await contentDigest(service.connection.endpoint),protocol_digest:request.protocol.digest,shape:request.shape,question_count:3,tested_at_unix_ms:Date.now(),duration_millis:1,reason:'complete'};
      };
      await click(t('测试此路由的适用性（1 次请求）','Test route applicability (1 request)'));
      await until(()=>text().includes(t('历史问题适用性测试通过','Historical problem applicability test passed')),'exact capability evidence');
      setInput(document.querySelector('.native-protocol-editor input[type="number"]'),'0');await until(()=>text().includes(t('背景为 0','With zero background')),'zero background');
      assert(text().includes(t('本次打开尚未测试历史问题适用性','Not tested in this session')),'Changed content reused evidence');
      setInput(document.querySelector('.native-protocol-editor input[type="number"]'),'8');
      await until(()=>!text().includes(t('历史问题适用性测试通过','Historical problem applicability test passed')),'Old evidence resurrected');assertCopy();geometry();
    }
  }),scenario('desktop.decisions.connectivity-test',['decision-services','c-protocol'],'The connection page retains its original single V1 connectivity test',async()=>{
    for(const language of ['zh','en']){
      await fresh(()=>{control().handlers.decision_services=()=>({services:[service]});control().handlers.test_classifier_decision=({input})=>{assert(input.classifier?.service.revision===7&&!input.request,'Connection switched contract');return {schema:'hiroute.classifier-decision-test-result/v1',outcome:'passed',duration_millis:1};};});
      localStorage.setItem('hiroute.language',language);control().remount();const t=(zh,en)=>language==='zh'?zh:en;
      await click(t('模型','Models'));await click(t('决策模型','Decision models'));await until(()=>document.querySelector('.decision-heading h2')?.textContent===service.name,'connection details');await click(t('测试连接','Test connection'));
      await until(()=>text().includes(t('连接测试通过','Connection test passed')),'connection test');
      assert(!all('select').some(e=>e.getAttribute('aria-label')?.includes('shape')),'Shape selector remains');assert(calls('test_classifier_decision').length===1,'Connection sent more than one request');assertCopy();
    }
  }),scenario('desktop.quality.c-observations', ['routing-editor', 'sessions', 'c-protocol'], 'Retained C counts, denominators, partial coverage and empty states remain readable', async () => {
    for (const language of ['zh', 'en']) {
      const t = (zh, en) => language === 'zh' ? zh : en;
      let partial = false, empty = false;
      const cohort = { protocol: 'hiroute.system-one-c/v1', content_digest: 'sha256:' + 'a'.repeat(64), bundle_digest: 'sha256:' + 'b'.repeat(64),
        scope_id: 'synthetic-category-with-long-visible-name', standard: 'custom', provider: 'fixture-provider', model: 'synthetic-c-model-with-long-name', simple_threshold_millis: 800, competence_floor_millis: 500 };
      await fresh(() => {
        control().handlers.observation_read = payload => {
          if (payload.request.intent.view !== 'plan_quality') return control().fixtureResponse('observation_read', payload);
          assert(payload.request.schema === 'hiroute.observation.query/v2' && payload.request.intent.query.include_routing_observations === true, 'Desktop did not use the current versioned query');
          return { samples: [], summary: { models: [], scored_stage_count: 0, unrated_stage_count: 0, session_count: 0, available_revisions: [8] }, next_cursor: null,
            routing_observations: { coverage: partial ? 'partial' : 'complete', covered_from_ms: 0, covered_to_ms: Date.now(), retained_decisions: empty ? 0 : 11,
              known_non_c_decisions: 1, legacy_or_unknown_decisions: partial ? 2 : 0, unknown_scope_decisions: 0, missing_projection_events: partial ? 1 : 0, scan_limited: partial,
              cohorts: empty ? [] : [{ cohort, counts: { c_decisions: 10, inherited: 1, requested: 7, unavailable: 4, no_target_omissions: 1, partial_target_omissions: 1,
                required_question_missing: 1, gate_opportunities: 4, suppressed: 1, unavailable_per_requested: partial ? null : 4 / 7,
                suppressed_per_opportunity: partial ? null : 1 / 4, suppressed_per_c_decision: partial ? null : 1 / 10 } }] } };
        };
      });
      localStorage.setItem('hiroute.language', language); control().remount();
      if (language === 'zh') await routing();
      else { await click('Smart routing'); await until(() => all('.master-list .list-row').length, 'saved routes'); all('.master-list .list-row')[0].click(); }
      await click(t('模型表现', 'Model performance'));
      await until(() => all('.routing-observations summary').length, 'C observation entry');
      all('.routing-observations summary')[0].click();
      await until(() => text().includes('57.1%') && text().includes('25.0%') && text().includes('10.0%'), 'retained denominator rates');
      assert(all('.routing-counts')[0].textContent.includes('4 / 7'), 'Unavailable numerator or request denominator lost');
      for (const element of all('.routing-counts dt, .routing-counts dd, .routing-cohort strong, .quality-diagnostic-identity')) {
        assert(element.scrollWidth <= element.clientWidth + 1, 'C observation identity or count overflows');
      }
      partial = true; all('.quality-refresh')[0].click();
      await until(() => text().includes(t('覆盖不完整，比例不可用', 'Partial coverage; rates unavailable')), 'partial coverage');
      assert(!all('.routing-counts')[0].textContent.includes('%') && all('.routing-counts')[0].textContent.includes('4 / 7'), 'Partial coverage hides counts or claims a rate');
      empty = true; partial = false; all('.quality-refresh')[0].click();
      await until(() => text().includes(t('此范围内暂无历史问题适用性决策记录', 'No historical problem applicability decisions are available')), 'empty retained scope');
    }
  })];
}
