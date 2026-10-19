#!/usr/bin/env python3
"""Offline research replay and evaluator. Python 3.11+, standard library only."""
from __future__ import annotations
import argparse,copy,gzip,hashlib,json,math,random,statistics,sys
from collections import Counter,defaultdict
from pathlib import Path

ROOT=Path(__file__).resolve().parent
sys.path.insert(0,str(ROOT/'analysis'))
from context_eval import summarize,paired_comparison,routing_rule,metric_valid,gold_for,validate_dataset
from methods import parse_prediction
from fair_methods import parse as parse_matched


def read(path):
    path=Path(path)
    if path.suffix=='.gz':
        with gzip.open(path,'rt',encoding='utf-8') as handle:return [json.loads(x) for x in handle if x.strip()]
    return [json.loads(x) for x in path.read_text().splitlines() if x.strip()]


def load(path):return json.loads(Path(path).read_text())
def save(path,value):Path(path).write_text(json.dumps(value,indent=2,ensure_ascii=False,allow_nan=False)+'\n')


def verify_manifest():
    manifest=load(ROOT/'MANIFEST.json')
    for item in manifest['files']:
        path=ROOT/item['path']
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest()!=item['sha256']:
            raise ValueError('Artifact hash mismatch: '+item['path'])
    return len(manifest['files'])


def group_name(job):
    method=job.get('method',job.get('encoding'))
    if job.get('direct_threshold') is not None:method+='@'+str(job['direct_threshold'])
    return job['backend']+'/'+method


def parse_event(study,job,event):
    out={**job,'valid':False,'metric_valid':{},'action':None}
    if not event:return {**out,'invalid_reason':'missing_result'}
    out['elapsed_ms']=event.get('elapsed_ms')
    try:
        if event.get('status')!=200:raise ValueError('transport_status')
        response=event['response']
        if response.get('model')!=study['expected_models'][job['backend']]:raise ValueError('model_identity')
        if study['kind'] in ('matched','expression','probes','aborted','format_development'):
            method=job.get('method',job.get('encoding'))
            if study['kind'] in ('expression','probes'):method='direct'
            out.update(parse_matched(response,job['backend'],method))
        else:out.update(parse_prediction(response,job['method'],threshold=.5))
        if not out.get('valid'):out['invalid_reason']='invalid_answer_schema'
    except (ValueError,KeyError,TypeError,IndexError,AttributeError,AssertionError) as exc:
        out.update(valid=False,action=None,metric_valid={},invalid_reason=type(exc).__name__+': '+str(exc)[:160])
    return out


def get_study(study):
    jobs=read(ROOT/study['jobs']);events=read(ROOT/study['responses'])
    if len(jobs)!=study['planned_calls'] or len({x['job_id'] for x in jobs})!=len(jobs):raise ValueError('Invalid frozen job plan')
    finals={}
    for event in events:
        if event.get('event')=='result' and event.get('final',True):
            key=event['job_id']
            if key in finals:raise ValueError('Duplicate final response: '+key)
            finals[key]=event
    if set(finals)-{j['job_id'] for j in jobs}:raise ValueError('Unplanned response')
    predictions=[parse_event(study,j,finals.get(j['job_id'])) for j in jobs]
    return jobs,events,predictions


def panels(labels):
    if not labels or 'evaluation_panel' not in labels[0]:return {'main':labels}
    main=[l for l in labels if l['evaluation_panel']=='main']
    out={'main':main,'expression':[l for l in labels if l['evaluation_panel']=='expression']}
    for name,cohort in [('original_main','original'),('coverage_supplement','coverage_supplement')]:
        selected=[l for l in main if l.get('construction_cohort')==cohort]
        if selected:out[name]=selected
    return {k:v for k,v in out.items() if v}


def metrics(labels,preds,repeats,selected_metrics=None):
    ids={l['id'] for l in labels};selected=[p for p in preds if p['id'] in ids]
    if selected_metrics is None:
        selected_metrics=tuple(k for k in ['current_demand','stage_grade','applicability','action'] if k=='action' or any(k in p for p in selected))
    return summarize(labels,selected,repeats=repeats,metrics=selected_metrics)


def evaluate_predictions(labels,predictions,repeats,typed=False):
    selected_metrics=('current_demand','stage_grade','applicability','action') if typed else ('action',)
    # Validate the complete submitted ID/repeat population before panel filtering.
    summarize(labels,predictions,repeats=repeats,metrics=selected_metrics)
    return {'track':'typed' if typed else 'action','panels':{name:metrics(ls,predictions,repeats,selected_metrics) for name,ls in panels(labels).items()}}



def correct(label,pred,field='action'):
    target=gold_for(label,field)
    return target is not None and metric_valid(pred,field) and type(pred[field]) is type(target) and pred[field]==target


def diagnostics(labels,preds,repeats):
    li={l['id']:l for l in labels};gate=Counter();partition=Counter();basis=Counter();common=[]
    derived={m:[] for m in ['current_only','without_applicability']}
    for p in preds:
        l=li[p['id']]
        if not p.get('valid') or not all(k in p for k in ['p_simple','historical_score','p_applicable']):continue
        s,h,a=p['p_simple'],p['historical_score'],p['p_applicable']
        if h is None or a is None:continue
        if l.get('applicability') in ('applicable','not_applicable') and p.get('applicability')!=l['applicability']:
            bucket=l.get('panel','non_determinate') if l['action_status']!='determinate' else 'current_masked' if s<.5 else 'score_masked' if h>=1 else 'action_changing'
            partition[bucket]+=1
        for mode in derived:
            action=routing_rule(s,h,None,threshold=.5,use_history=mode!='current_only',use_applicability=False)
            derived[mode].append({'id':p['id'],'repeat':p['repeat'],'valid':True,'action':action})
            if mode=='without_applicability' and l['action_status']=='determinate' and action!=p['action']:
                gate['corrected' if correct(l,p) else 'regressed']+=1
                if correct(l,p):
                    kind='released_obsolete_failure' if l['stage_grade']==0 and l['applicability']=='not_applicable' else 'blocked_wrong_low_score' if l['stage_grade']==2 and l['applicability']=='not_applicable' else 'other_or_uncertain'
                    basis[kind]+=1
    common=[l for l in labels if l['action_status']=='determinate' and l.get('applicability') in ('applicable','not_applicable')]
    eligible={l['id'] for l in labels}
    return {'applicability_error_partition':dict(partition),'gate_changes':dict(gate),'gate_correction_basis':dict(basis),
            'same_response_ablations':{k:metrics(labels,v,repeats) for k,v in derived.items()},
            'common_cohort':metrics(common,[p for p in preds if p['id'] in {l['id'] for l in common}],repeats) if common else None}


def bootstrap_comparison(labels,left,right,repeats,math_style=False):
    result=paired_comparison(labels,left,right,repeats=repeats)
    if math_style:
        # The mathematical pre-call analysis sorts families and uses order statistics.
        groups=defaultdict(list);a={(p['id'],p['repeat']):p for p in left};b={(p['id'],p['repeat']):p for p in right}
        for lab in labels:
            if lab['action_status']!='determinate':continue
            for repeat in range(repeats):groups[lab['family_id']].append(int(correct(lab,b.get((lab['id'],repeat),{})))-int(correct(lab,a.get((lab['id'],repeat),{}))))
        values=[statistics.mean(v) for _,v in sorted(groups.items())];rng=random.Random(20261018)
        samples=sorted(sum(rng.choices(values,k=len(values)))/len(values) for _ in range(10000))
        result['ci95']=[samples[249],samples[9749]]
        result['bootstrap_convention']='sorted families; 10,000 draws; order statistics 249/9749; seed 20261018'
    return result


def cohort_report(labels,preds,repeats):
    filters={
        'related_core_failure':lambda l:l['current_demand']=='routine' and l['stage_grade']==0 and l['applicability']=='applicable' and l['action']=='primary',
        'obsolete_core_failure':lambda l:l['current_demand']=='routine' and l['stage_grade']==0 and l['applicability']=='not_applicable' and l['action']=='economy',
        'healthy_history':lambda l:l['current_demand']=='routine' and l['stage_grade']==2 and l['applicability']=='not_applicable' and l['action']=='economy',
        'current_reasoning':lambda l:l['current_demand']=='reasoning'}
    return {k:metrics([l for l in labels if test(l)],preds,repeats)['metrics']['action'] for k,test in filters.items()}


def resource_report(events):
    cells=defaultdict(list)
    for e in events:
        if e['event']=='result':cells[group_name(e)].append(e)
    out={}
    for key,rs in cells.items():
        charges=[r['response'].get('usage',{}).get('cost') for r in rs if isinstance(r.get('response'),dict)]
        charges=[c for c in charges if type(c) in (float,int)]
        times=sorted(r['elapsed_ms'] for r in rs if r.get('status')==200 and type(r.get('elapsed_ms')) in (float,int))
        usage=[r['response'].get('usage',{}) for r in rs if isinstance(r.get('response'),dict)]
        out[key]={'observed_results':len(rs),'known_charge_calls':len(charges),'usage_charge_usd':sum(charges),'usd_per_1000_requests':sum(charges)/len(rs)*1000 if len(charges)==len(rs) else None,
                  'input_tokens':sum(u.get('input_tokens',u.get('prompt_tokens',0)) for u in usage),
                  'output_tokens':sum(u.get('output_tokens',u.get('completion_tokens',0)) for u in usage),
                  'median_ms':statistics.median(times) if times else None,'p95_nearest_rank_ms':times[math.ceil(.95*len(times))-1] if times else None}
    return out


def study_report(study):
    jobs,events,preds=get_study(study)
    result={'execution':study,'observed_valid':sum(p['valid'] for p in preds),'invalid_or_missing':[{k:p.get(k) for k in ['job_id','invalid_reason']} for p in preds if not p['valid']], 'resources':resource_report(events)}
    if study['kind'] in ('aborted','probes','format_development'):
        if study['kind']=='probes':
            labels={l['id']:l for l in read(ROOT/'datasets/direct-probes/probes.jsonl')};counts=defaultdict(Counter)
            for p in preds:
                expected=labels[p['id']]['expected_action'];c=counts[group_name(p)]
                c['boundary' if expected is None else 'determinate']+=1
                if expected is not None:c['correct']+=int(p['valid'] and p['action']==expected)
            result['probe_results']={k:dict(v) for k,v in counts.items()}
        return result,preds
    labels=read(ROOT/'datasets'/study['dataset']/'labels.jsonl');groups=defaultdict(list)
    for p in preds:groups[group_name(p)].append(p)
    result['panels']={}
    for name,ls in panels(labels).items():
        block={'labels':dict(Counter(l['panel'] for l in ls)),'methods':{k:metrics(ls,v,study['repeats']) for k,v in groups.items()},'comparisons':{},'cohorts':{},'diagnostics':{}}
        if name=='main':
            for key,ps in groups.items():
                ids={l['id'] for l in ls};ps=[p for p in ps if p['id'] in ids]
                block['cohorts'][key]=cohort_report(ls,ps,study['repeats'])
                if key.endswith(('/decomposed','/c_three')):block['diagnostics'][key]=diagnostics(ls,ps,study['repeats'])
            for b in ['jev113','jev114','qwen38flash']:
                a,z=groups.get(b+'/direct'),groups.get(b+'/decomposed')
                if a and z:block['comparisons'][b+'/three_minus_direct']=bootstrap_comparison(ls,[p for p in a if p['id'] in ids],[p for p in z if p['id'] in ids],study['repeats'],study['dataset']=='mathematics')
                if study['kind']=='expression' and b+'/original' in groups:
                    block['comparisons'][b+'/explicit_minus_original']=bootstrap_comparison(ls,[p for p in groups[b+'/original'] if p['id'] in ids],[p for p in groups[b+'/explicit'] if p['id'] in ids],2,True)
                if study['kind']=='independent' and b+'/c_three' in groups:
                    for m in ['c_two','legacy_full']:
                        block['comparisons'][b+'/three_minus_'+m]=bootstrap_comparison(ls,[p for p in groups[b+'/'+m] if p['id'] in ids],[p for p in groups[b+'/c_three'] if p['id'] in ids],2)
        result['panels'][name]=block
    if any(l.get('evaluation_panel')=='expression' for l in labels):
        result['expression_stability']={}
        for key,ps in groups.items():
            pi={(p['id'],p['repeat']):p for p in ps};li={l['id']:l for l in labels};counts=Counter()
            for l in labels:
                if l.get('evaluation_panel')!='expression':continue
                for repeat in range(study['repeats']):
                    a=pi.get((l['id'],repeat),{});b=pi.get((l['equivalent_to'],repeat),{})
                    valid=metric_valid(a,'action') and metric_valid(b,'action')
                    counts['planned_pairs']+=1;counts['both_valid']+=int(valid)
                    counts['action_flips']+=int(valid and a['action']!=b['action'])
                    original=li[l['equivalent_to']]
                    if l['action_status']==original['action_status']=='determinate':
                        counts['determinate_pairs']+=1;counts['both_correct']+=int(correct(l,a) and correct(original,b))
            result['expression_stability'][key]=dict(counts)
    if study['dataset']=='mathematics':
        alternative=copy.deepcopy(labels);overlay=load(ROOT/'sensitivities/mr010.json')
        for change in overlay['changes']:
            for l in alternative:
                if l['id']==change['id']:l.update({k:v for k,v in change.items() if k!='id'})
        result['mr010_sensitivity']={k:metrics(alternative,v,study['repeats']) for k,v in groups.items()}
    return result,preds


def markdown_summary(report):
    text=['# Offline reproduction','', 'All results below are recomputed from public responses and unchanged labels. No model calls.','']
    for sid,s in report['studies'].items():
        text+=['## '+sid,'',f"Planned: {s['execution']['planned_calls']}; valid: {s['observed_valid']}; invalid or missing: {len(s['invalid_or_missing'])}.",'']
        if 'panels' in s:
            text+=['| Method | Action macro | Correct / planned clear calls |','|---|---:|---:|']
            for name,v in s['panels']['main']['methods'].items():
                m=v['metrics']['action'];text.append(f"| {name} | {100*m['family_accuracy']:.5f}% | {m['correct_calls']} / {m['eligible_calls']} |")
            text.append('')
        if 'probe_results' in s:text+=['```json',json.dumps(s['probe_results'],indent=2),'```','']
    return '\n'.join(text)+'\n'


def check_references(report):
    expected=load(ROOT/'reference-results.json');checked=0
    def equal(actual,target):
        if type(target) in (int,float):return type(actual) in (int,float) and abs(actual-target)<1e-11
        return actual==target
    for sid,reference in expected['studies'].items():
        result=report['studies'][sid]['panels']['main']
        for key,metrics_ in reference['action'].items():
            for field,target in metrics_.items():
                actual=result['methods'][key]['metrics']['action'][field]
                if not equal(actual,target):raise ValueError(f'Reference mismatch: {sid}/{key}/{field}: {actual} != {target}')
                checked+=1
        for key,metrics_ in reference.get('components',{}).items():
            for field,target in metrics_.items():
                actual=result['methods'][key]['metrics'][field]['family_accuracy']
                if not equal(actual,target):raise ValueError('Component reference mismatch: '+sid+'/'+key+'/'+field)
                checked+=1
        for key,fields in reference.get('resources',{}).items():
            for field,target in fields.items():
                actual=report['studies'][sid]['resources'][key][field]
                if not equal(actual,target):raise ValueError('Resource reference mismatch: '+sid+'/'+key+'/'+field)
                checked+=1
        for backend,reference_ in reference.get('comparison',{}).items():
            actual=result['comparisons'][backend+'/three_minus_direct']
            if not equal(actual['difference'],reference_['difference']) or any(not equal(a,b) for a,b in zip(actual['ci95'],reference_['ci95'])):raise ValueError('Interval mismatch: '+backend)
            checked+=3
    expected_sensitivity={'jev113/direct':.9375,'jev113/decomposed':.96875,'jev114/direct':.921875,'jev114/decomposed':.9765625,'qwen38flash/direct':.890625,'qwen38flash/decomposed':.953125}
    for key,target in expected_sensitivity.items():
        actual=report['studies']['mathematics-matched']['mr010_sensitivity'][key]['metrics']['action']['family_accuracy']
        if not equal(actual,target):raise ValueError('Sensitivity mismatch: '+key)
        checked+=1
    return {'passed_numeric_comparisons':checked,'source':'reference-results.json','sensitivity_separate':True}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('command',choices=['verify','report','case','score'])
    parser.add_argument('--output',type=Path);parser.add_argument('--check',action='store_true',help='Compare recomputed results against separate frozen reference reductions');parser.add_argument('--dataset',choices=['engineering','mathematics','development-v3'],default='engineering')
    parser.add_argument('--id');parser.add_argument('--typed',action='store_true',help='Evaluate component answers in addition to the generic action track');parser.add_argument('--predictions',type=Path);parser.add_argument('--repeats',type=int,default=1)
    args=parser.parse_args();verified=verify_manifest();studies=load(ROOT/'execution/studies.json')
    if args.command=='verify':
        ds={}
        for name in ['engineering','mathematics','development-v3']:
            inputs=read(ROOT/'datasets'/name/'inputs.jsonl');labels=read(ROOT/'datasets'/name/'labels.jsonl')
            core_inputs=[{'id':r['id'],'conversation':r['conversation']} for r in inputs]
            ds[name]=validate_dataset(core_inputs,labels)
        print(json.dumps({'verified_files':verified,'datasets':ds},indent=2));return
    if args.command=='score':
        if args.predictions is None:parser.error('--predictions is required')
        print(json.dumps(evaluate_predictions(read(ROOT/'datasets'/args.dataset/'labels.jsonl'),read(args.predictions),args.repeats,args.typed),indent=2));return
    if args.command=='case':
        if not args.id:parser.error('--id is required')
        cases=[x for x in read(ROOT/'datasets'/args.dataset/'inputs.jsonl') if x['id']==args.id]
        labels=[x for x in read(ROOT/'datasets'/args.dataset/'labels.jsonl') if x['id']==args.id]
        if not cases:parser.error('Case not found')
        outputs={}
        for study in studies:
            if study['dataset']==args.dataset and study['kind']!='aborted':
                _,_,ps=get_study(study);outputs[study['id']]=[p for p in ps if p['id']==args.id]
        print(json.dumps({'inputs':cases,'labels':labels,'parsed_returns':outputs},ensure_ascii=False,indent=2));return
    report={'format_version':1,'verified_files':verified,'simple_threshold':.5,'history_threshold_strict':1,'applicability_threshold':.5,'studies':{}}
    if args.output:
        args.output.mkdir(parents=True,exist_ok=True)
        if (args.output/'report.json').exists():raise ValueError('Choose a fresh output directory')
    for study in studies:
        print('Replaying '+study['id'],file=sys.stderr)
        data,predictions=study_report(study);report['studies'][study['id']]=data
        if args.output:
            raw=''.join(json.dumps(p,ensure_ascii=False,separators=(',',':'))+'\n' for p in predictions).encode()
            (args.output/(study['id']+'-predictions.jsonl.gz')).write_bytes(gzip.compress(raw,mtime=0))
    if args.check:
        report['reference_checks']=check_references(report)
    if args.output:
        save(args.output/'report.json',report);(args.output/'RESULTS.md').write_text(markdown_summary(report))
    print(markdown_summary(report))

if __name__=='__main__':main()
