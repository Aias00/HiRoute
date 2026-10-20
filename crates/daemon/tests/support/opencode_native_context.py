"""OpenCode native locations/records; shared product journeys own business assertions."""
from copy import deepcopy
import json
from pathlib import Path
from native_context_fixture import write_new, protect_configuration

def select_context(product):
    home=Path(product.env['HOME'])
    for key,folder in [('XDG_CONFIG_HOME','.config'),('XDG_DATA_HOME','.local/share'),('XDG_CACHE_HOME','.cache'),('XDG_STATE_HOME','.local/state')]:
        root=home/folder
        root.mkdir(mode=0o700,parents=True,exist_ok=True)
        product.env[key]=str(root)
    root=Path(product.env['XDG_CONFIG_HOME'])/'opencode'
    root.mkdir(mode=0o700,exist_ok=True)
    product.env.update(OPENCODE_DISABLE_MODELS_FETCH='true',OPENCODE_DISABLE_AUTOUPDATE='true',OPENCODE_DISABLE_LSP_DOWNLOAD='true',OPENCODE_DISABLE_FFF='true',OPENCODE_PURE='true',OPENCODE_DISABLE_DEFAULT_PLUGINS='true')
    product.env['OPENCODE_DB']=str(Path(product.env['XDG_DATA_HOME'])/'opencode/opencode.db')
    return root

def prepare_context(product,fixture):
    root=Path(fixture['config'])
    fixture['product_storage']=str(product.storage)
    fixture['database']=product.env['OPENCODE_DB']
    import secrets
    nonce=secrets.token_hex(10)
    fixture['edit']={'path':str(product.project/'opencode-owned-edit.txt'),'before':'before-'+nonce,'after':'edited-'+nonce}
    write_new(Path(fixture['edit']['path']),fixture['edit']['before']+'\n')
    write_new(root/'opencode.json',json.dumps({'$schema':'https://opencode.ai/config.json','model':'ambient/wrong-native-model','small_model':'ambient/wrong-native-model','provider':{'ambient':{'npm':'@ai-sdk/openai','options':{'baseURL':'http://127.0.0.1:9/v1','apiKey':'wrong-native-key'},'models':{'wrong-native-model':{'limit':{'context':32768,'output':4096}}}}},'share':'disabled','autoupdate':False}))
    protect_configuration(fixture,[root/'opencode.json'])

def exact_history(fixture,cancelled_session_id=None):
    import sqlite3
    matches=[]
    for marker in (Path(fixture['product_storage'])/'delegation-workers/sessions').glob('*/.hiroute-native-root-v1.json'):
        value=json.loads(marker.read_text())
        if value['task_id']!=fixture['task_id']: continue
        assert value['harness']=='opencode'
        binding={'native_session_id':cancelled_session_id} if cancelled_session_id else json.loads((marker.parent/'native-session-binding.json').read_text())
        database=Path(fixture['database'])
        assert database.is_file() and not database.is_symlink()
        # Only the selected synthetic session's existence/identity; production never parses its history.
        with sqlite3.connect(database.as_uri()+'?mode=ro',uri=True) as db:
            assert db.execute('SELECT id FROM session WHERE id=?',(binding['native_session_id'],)).fetchone()==(binding['native_session_id'],)
        matches.append((str(database),binding['native_session_id']))
    assert len(matches)==1,'expected exactly one bound OpenCode task'
    return matches[0]

class OpenCodeModelSettings:
    def __init__(self,config,foreign_endpoint):
        self.harness,self.auth_owned='opencode',None
        self.path=Path(config)/'opencode.json'
        assert not self.path.exists(),'model journey requires its dedicated empty context'
        self.original=None
        self.native_default='fixture-native/native-default'
        self.path.write_text(json.dumps({'$schema':'https://opencode.ai/config.json','model':self.native_default,'small_model':self.native_default,'provider':{'fixture-native':{'npm':'@ai-sdk/openai','options':{'baseURL':foreign_endpoint,'apiKey':'synthetic-unselected-native-key'},'models':{'native-default':{'limit':{'context':100000,'output':4096}}}}},'share':'disabled','autoupdate':False,'compaction':{'auto':False},'agent':{'title':{'disable':True},'summary':{'disable':True}}}))
        self.path.chmod(0o600)
        self.baseline=self.read()
    def read(self):
        assert self.path.is_file() and not self.path.is_symlink()
        value=json.loads(self.path.read_text())
        value['providers']=value.pop('provider',{})
        for provider in value['providers'].values():
            provider['models']=[dict(model,id=key) for key,model in provider['models'].items()]
        value['model']={'name':value['model']}
        return value
    def assert_preserved(self,provider_id,selected_default=None):
        actual=self.read()
        for name in list(actual['providers']):
            if name==provider_id or name.startswith(provider_id+'-'): actual['providers'].pop(name)
        expected=deepcopy(self.baseline)
        if selected_default: expected['model']['name']=selected_default
        assert actual==expected,'OpenCode save changed daily defaults/unowned fields'
    def add_user_edit(self,provider_id):
        self.assert_preserved(provider_id)
        text=self.path.read_text();end=text.rfind('}')
        self.path.write_text(text[:end].rstrip()+',"instructions":["./user-rule.md"]'+text[end:])
        self.baseline['instructions']=['./user-rule.md']
    def select_default(self,expected,replacement):
        text=self.path.read_text();needle=json.dumps(expected)
        assert self.read()['model']['name']==expected
        # Only the root value is replaced; owned provider bytes stay unchanged.
        start=text.index('"model"');at=text.index(needle,start)
        self.path.write_text(text[:at]+json.dumps(replacement)+text[at+len(needle):])
    def close(self):
        assert self.read()==self.baseline
        self.path.unlink()

def main_route(product,endpoint,model,token,fixture):
    config=Path(fixture['config'])/'opencode.json'
    value=json.loads(config.read_text())
    value['provider']['hiroute-main-acceptance']={'npm':'@ai-sdk/openai','options':{'baseURL':endpoint,'apiKey':token,'store':False},'models':{model:{'limit':{'context':100000,'output':4096}}}}
    value['model']=value['small_model']='hiroute-main-acceptance/'+model
    value['permission']={'*':'allow'}
    value['agent']={'title':{'disable':True},'summary':{'disable':True}}
    config.write_text(json.dumps(value));config.chmod(0o600)
    protect_configuration(fixture,[config])
    return 'hiroute-main-acceptance/'+model

def cancelled_history(fixture,run_id):
    from native_task_witness import cancelled_session
    return exact_history(fixture,cancelled_session(fixture,run_id,'opencode'))
