import {writeFileSync} from 'node:fs';
import {relative} from 'node:path';
export function productionBoundary(root,state) {
  function checkModule(id) {
    const name=id.replaceAll('\\','/');
    if(name.includes('/devdemo/')||name.includes('/vite/dist/client/'))throw new Error('Development module in production output');
  }
  return {name:'production-dashboard-boundary',moduleParsed(module){checkModule(module.id);},generateBundle(_options,bundle){
    const modules=new Set();
    for(const item of Object.values(bundle)) {
      if(item.type==='chunk')for(const id of Object.keys(item.modules)) {
        checkModule(id);
        modules.add(relative(root,id).replaceAll('\\','/'));
      }
      const text=item.type==='chunk'?item.code:String(item.source);
      if(/gateway-devdemo\/v1|synthetic-browser-read-key|\/@vite\/client|vite-hmr/.test(text))throw new Error('Development content in production output');
    }
    writeFileSync(state+'production-modules.json',JSON.stringify([...modules].sort(),null,2)+'\n');
  }};
}
