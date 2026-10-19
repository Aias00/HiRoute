"""Contract tests for invalid accounting, boundaries and continuous-score routing."""
import copy,gzip,importlib.util,json,sys,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT));sys.path.insert(0,str(ROOT/'analysis'))
import reproduce
from context_eval import summarize,routing_rule


class ReplayContracts(unittest.TestCase):
    def label(self,ident='case',action='primary',status='determinate'):
        return {'id':ident,'family_id':'family','current_demand':'routine','stage_grade':0,'applicability':'applicable','action_status':status,'action':action,'panel':'clear' if status=='determinate' else 'boundary'}

    def test_missing_and_invalid_stay_in_planned_denominator(self):
        labels=[self.label()]
        score=summarize(labels,[{'id':'case','repeat':0,'valid':False,'action':'primary'}],repeats=2,metrics=('action',))
        self.assertEqual(score['metrics']['action']['eligible_calls'],2)
        self.assertEqual(score['metrics']['action']['correct_calls'],0)
        self.assertEqual(score['missing_responses'],1)

    def test_boundary_has_no_forced_action_credit(self):
        l=self.label(action=None,status='conditional')
        score=summarize([l],[{'id':'case','valid':True,'action':'primary'}],metrics=('action',))
        self.assertEqual(score['metrics']['action']['eligible_calls'],0)
        self.assertEqual(score['non_determinate_actions'],{'primary':1})

    def test_continuous_score_is_not_modal_grade(self):
        self.assertEqual(routing_rule(.95,.95,.9),'primary')
        self.assertEqual(routing_rule(.95,1,.9),'economy')
        self.assertEqual(routing_rule(.95,.95,.49),'economy')

    def test_duplicates_are_not_silently_last_wins(self):
        p={'id':'case','valid':True,'action':'primary'}
        with self.assertRaises(ValueError):summarize([self.label()],[p,p],metrics=('action',))

    def test_model_mismatch_is_an_end_to_end_failure(self):
        study={'kind':'matched','expected_models':{'jev113':'expected'}}
        job={'id':'case','job_id':'case/jev113/direct/0','backend':'jev113','method':'direct','repeat':0}
        p=reproduce.parse_event(study,job,{'status':200,'response':{'model':'different'}})
        self.assertFalse(p['valid']);self.assertIsNone(p['action'])

    def test_invalid_saved_qwen_output_is_preserved(self):
        study=next(s for s in reproduce.load(ROOT/'execution/studies.json') if s['id']=='mathematics-matched')
        _,_,ps=reproduce.get_study(study)
        bad=[p for p in ps if not p['valid']]
        self.assertEqual(len(bad),1);self.assertEqual(bad[0]['id'],'MR008_a_zh')
        self.assertEqual(bad[0]['backend'],'qwen38flash')

    def test_sensitive_overlay_does_not_modify_original_labels(self):
        original=reproduce.read(ROOT/'datasets/mathematics/labels.jsonl');before=copy.deepcopy(original)
        alternate=copy.deepcopy(original)
        for change in reproduce.load(ROOT/'sensitivities/mr010.json')['changes']:
            for label in alternate:
                if label['id']==change['id']:label.update({k:v for k,v in change.items() if k!='id'})
        self.assertEqual(original,before)
        self.assertEqual(sum(l['action_status']=='determinate' for l in original),50)
        self.assertEqual(sum(l['action_status']=='determinate' for l in alternate),52)

    def test_generic_track_keeps_expression_separate_and_omits_component_penalties(self):
        main={**self.label('main'),'evaluation_panel':'main','construction_cohort':'original'}
        expression={**self.label('rewrite'),'evaluation_panel':'expression','construction_cohort':'original'}
        ps=[{'id':'main','repeat':0,'valid':True,'action':'primary'},{'id':'rewrite','repeat':0,'valid':True,'action':'economy'}]
        result=reproduce.evaluate_predictions([main,expression],ps,1)
        self.assertEqual(result['panels']['main']['metrics']['action']['family_accuracy'],1)
        self.assertEqual(result['panels']['expression']['metrics']['action']['family_accuracy'],0)
        self.assertEqual(set(result['panels']['main']['metrics']),{'action'})

    def test_unknown_external_ids_are_rejected_before_panel_filtering(self):
        with self.assertRaises(ValueError):
            reproduce.evaluate_predictions([self.label()],[{'id':'unplanned','repeat':0,'valid':True,'action':'primary'}],1)

    def test_compressed_response_is_not_an_opaque_score_cache(self):
        events=reproduce.read(ROOT/'responses/mathematics-matched.jsonl.gz')
        outputs=[e for e in events if e['event']=='result']
        self.assertEqual(len(outputs),768)
        self.assertTrue(all('answers' in e['response'] or 'choices' in e['response'] for e in outputs))

if __name__=='__main__':unittest.main()
