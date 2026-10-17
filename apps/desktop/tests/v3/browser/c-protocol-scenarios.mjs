import { scenario } from './scenario-selection.mjs';

export function cProtocolScenarios({ fresh, routing, click, until, all, text, calls, setInput, control }) {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const choose = (select, value) => { select.value = value; select.dispatchEvent(new Event('change', { bubbles: true })); };
  const service = { id: 'decision/c-ui-fixture', revision: 7, name: 'Saved C fixture', connection: {
    kind: 'system_one', provider: 'compatible', model: 'synthetic-c-model', endpoint: 'https://decision.example/systemone', timeout_ms: 3000,
    auth_header: { name: 'Authorization', value_secret_ref: 'decision/c-ui-fixture/r7' },
  } };
  return [scenario('desktop.routing.c-scope-review', ['routing-editor', 'decision-services', 'c-protocol'], 'C scope adoption, session evidence, rollback and long composed text remain usable', async () => {
    for (const language of ['zh', 'en']) {
      const t = (zh, en) => language === 'zh' ? zh : en;
      await fresh(() => { control().handlers.decision_services = () => ({ services: [service] }); });
      localStorage.setItem('hiroute.language', language); control().remount();
      if (language === 'zh') await routing();
      else {
        await click('Smart routing');
        await until(() => all('.master-list .list-row').length, 'saved routes'); all('.master-list .list-row')[0].click();
        await until(() => document.querySelector('.plan-identity-fields input'), 'English route editor');
      }
      await click(t('决策模型', 'Decision model'));
      await until(() => document.querySelector('.native-protocol-editor select'), 'native protocol selector');
      const select = () => document.querySelector('.native-protocol-editor select');
      assert(select().value === 'legacy', 'Existing route automatically adopted C');
      choose(select(), 'c');
      await until(() => all('[data-native-scope="smart_saving"]').length, 'effective C scope');
      assert(text().includes(t('本次打开尚未测试 C 能力', 'Not tested in this session')), 'Missing session evidence is not explicit');
      await click(t('采用标准 C 评分', 'Adopt stock C assessment'));
      const review = () => document.querySelector('[data-native-scope="smart_saving"] details');
      review().querySelector('summary').click();
      await until(() => review().querySelector('code')?.textContent.startsWith('sha256:'), 'composed review digest');
      assert(review().textContent.includes('state.assessment_stage') && review().querySelectorAll('.native-prompt').length === 4, 'Incomplete composed question or criteria');
      await click(t('已审阅，确认此范围', 'Reviewed: confirm this scope'));
      await until(() => text().includes(t('已审阅当前协议与完整评分标准', 'Current protocol and composed assessment reviewed')), 'scope review completed');
      assert(calls('test_classifier_decision').length === 0, 'Adoption implicitly tested a model');
      control().handlers.test_classifier_decision = async ({ input }) => {
        const request = input.request;
        assert(request.service_revision === 7 && request.shape === 'standard', 'Probe lost exact saved revision or shape');
        const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify(service.connection.endpoint)));
        return { schema: 'hiroute.classifier-decision-test-result/v2', state: 'verified', service_id: service.id, service_revision: 7,
          model: service.connection.model, endpoint_digest: 'sha256:' + [...new Uint8Array(bytes)].map(b => b.toString(16).padStart(2, '0')).join(''),
          protocol_digest: request.protocol.digest, shape: 'standard', question_count: 3, tested_at_unix_ms: Date.now(), duration_millis: 1, reason: 'complete' };
      };
      await click(t('测试此路由的 C 能力（1 次请求）', 'Test this route’s C capability (1 request)'));
      await until(() => text().includes(t('C 能力已验证', 'C capability verified')), 'exact-session capability result');
      setInput(document.querySelector('.native-protocol-editor input[type="number"]'), '0');
      await until(() => text().includes(t('背景为 0', 'With zero background')), 'zero-background warning');
      assert(text().includes(t('本次打开尚未测试 C 能力', 'Not tested in this session')), 'Changed content reused capability evidence');
      await click(t('保存草稿', 'Save draft'));
      await until(() => calls('preview_plan_editor').length, 'C draft boundary');
      const saved = calls('preview_plan_editor').at(-1).payload.input.editor.smart;
      assert(saved.native_protocol.prior_context_max_turns === 0 && saved.assessment_review.smart_saving.protocol === 'hiroute.system-one-c/v1', 'C draft lost scope review');
      assert(saved.judgment.degree.simple_threshold_millis === 800 && saved.judgment.competence.floor_millis === 500, 'Adoption changed numeric thresholds');
      choose(select(), 'legacy');
      await click(t('采用旧默认评分', 'Adopt legacy default assessment'));
      if (!review().open) review().querySelector('summary').click();
      await click(t('已审阅，确认此范围', 'Reviewed: confirm this scope'));
      await until(() => !all('.operation-feedback').some(e => e.textContent.includes('Saving')), 'draft settles');
      await click(t('保存草稿', 'Save draft'));
      await until(() => calls('preview_plan_editor').at(-1).payload.input.editor.smart.assessment_review.smart_saving.protocol === 'hiroute.system-one-legacy/v1', 'destination-specific rollback review');
      assert(!calls('preview_plan_editor').at(-1).payload.input.editor.smart.native_protocol, 'Rollback retained C execution mode');
      assert(calls('test_classifier_decision').length === 1, 'Draft save or rollback dispatched another probe');
      // Leave the full C composition open for host screenshot inspection.
      choose(select(), 'c'); await click(t('采用标准 C 评分', 'Adopt stock C assessment'));
      if (!review().open) review().querySelector('summary').click();
      review().scrollIntoView({ block: 'center' });
      await until(() => review().querySelector('code')?.textContent.startsWith('sha256:'), 'visible review content');
      for (const element of all('.native-protocol-editor .field-label, .native-protocol-editor .native-prompt, .native-content-digest, .native-protocol-editor button')) {
        assert(element.scrollWidth <= element.clientWidth + 1, 'C text or action overflows horizontally');
        if (element.classList.contains('field-label')) assert(element.getBoundingClientRect().width >= 160, 'C label collapsed under control pressure');
      }
      const footer = document.querySelector('.plan-editor-footer, .editor-actions, .plan-editor .modal-foot');
      if (footer) assert(footer.getBoundingClientRect().bottom <= innerHeight + 1, 'Save actions left the supported viewport');
    }
  })];
}
