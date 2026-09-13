import {createServer} from 'vite';
import vue from '@vitejs/plugin-vue';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';
const site=fileURLToPath(new URL('../',import.meta.url));
export function validPort(value) {
  const port=Number(value);
  if(!Number.isInteger(port)||port<1024||port>65535)throw new Error('Use a port from 1024 to 65535');
  return port;
}
export function requestBoundary(request,authority,upgrade=false) {
  const count=name=>request.rawHeaders.filter((_,i)=>i%2===0&&request.rawHeaders[i].toLowerCase()===name).length;
  if(count('host')!==1||request.headers.host!==authority)return false;
  const origin=request.headers.origin;
  return count('origin')<=1 && (!upgrade&&!origin || origin==='http://'+authority);
}
export async function startDemoServer({port=43142}={}) {
  if(process.versions.node!=='24.21.0')throw new Error('Use Node 24.21.0');
  port=validPort(port);
  const authority='127.0.0.1:'+port;
  const server=await createServer({
    root:join(site,'devdemo'),configFile:false,envFile:false,envPrefix:[],publicDir:false,appType:'mpa',
    cacheDir:join(site,'node_modules/.vite/devdemo-'+port),clearScreen:false,
    plugins:[{name:'devdemo-boundary',configureServer(server){
      server.middlewares.use((request,response,next)=>{
        if(!requestBoundary(request,authority)){response.writeHead(403).end();return;}
        if(!['GET','HEAD'].includes(request.method)){response.writeHead(405).end();return;}
        response.setHeader('Cache-Control','no-store');
        response.setHeader('Referrer-Policy','no-referrer');
        response.setHeader('X-Content-Type-Options','nosniff');
        response.setHeader('Content-Security-Policy',"default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ws://"+authority+"; img-src 'self' data:; frame-src 'self'; frame-ancestors 'self'; base-uri 'none'; form-action 'self'");
        if(request.url.startsWith('/management/')||request.url.startsWith('/__devdemo/')){response.writeHead(404).end();return;}
        next();
      });
    }},vue()],
    server:{host:'127.0.0.1',port,strictPort:true,open:false,cors:false,allowedHosts:['127.0.0.1'],
      fs:{strict:true,allow:[join(site,'devdemo'),join(site,'src'),join(site,'node_modules')],deny:['**/.git/**','**/.git','**/.private/**',...['devdemo','src','node_modules'].map(name=>join(site,name,'**/.local/**').replaceAll('\\','/')),'**/.env','**/.env.*','**/*.{pem,key}']}},
  });
  server.httpServer.prependListener('upgrade',(request,socket)=>{
    if(!requestBoundary(request,authority,true)){socket.destroy();}
  });
  await server.listen();
  return {url:'http://'+authority+'/',close:()=>server.close()};
}
