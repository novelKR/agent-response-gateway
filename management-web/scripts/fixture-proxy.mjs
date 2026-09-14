import {request as httpRequest,Agent} from 'node:http';
export function createFixtureProxy(fixture) {
  const origin=new URL(fixture.origin);
  if(origin.protocol!=='http:'||origin.hostname!=='127.0.0.1'||origin.username||origin.password||origin.search||origin.hash||origin.pathname!=='/')throw new Error('Invalid owned fixture endpoint');
  let cookieName;
  const agent=new Agent({keepAlive:false,proxyEnv:{}});
  function fail(response) {
    if(response.headersSent){response.destroy();return;}
    response.writeHead(503,{'Content-Type':'application/json','Cache-Control':'no-store'});
    response.end(JSON.stringify({schema:'gateway-management-http/v1',observed_at_ms:Date.now(),error:{code:'connection_unavailable'}}));
  }
  return (request,response)=>{
    const path=request.url.split('?')[0];
    const read=request.method==='GET'&&/^\/management\/v1\/(?:capabilities|state|usage|operations(?:\/[A-Za-z0-9_-][A-Za-z0-9_.:-]*)?)$/.test(path);
    const session=['POST','DELETE'].includes(request.method)&&path==='/management/v1/session';
    if(!read&&!session){response.writeHead(404).end();return;}
    if(session&&(!request.headers.origin||request.headers['transfer-encoding']||Number(request.headers['content-length']||0)!==0)){response.writeHead(403).end();return;}
    const cookies=request.rawHeaders.filter((_,i)=>i%2===0&&request.rawHeaders[i].toLowerCase()==='cookie').length;
    const authorizations=request.rawHeaders.filter((_,i)=>i%2===0&&request.rawHeaders[i].toLowerCase()==='authorization').length;
    if(cookies>1||authorizations>1||(request.headers.cookie?.length||0)>8192){response.writeHead(403).end();return;}
    const headers={Accept:'application/json',Host:origin.host};
    if(request.headers.origin)headers.Origin=origin.origin;
    if(request.method==='POST'&&request.headers.authorization)headers.Authorization=request.headers.authorization;
    if(cookieName&&request.headers.cookie) {
      const own=request.headers.cookie.split(';').map(value=>value.trim()).filter(value=>value.startsWith(cookieName+'='));
      if(own.length>1){response.writeHead(403).end();return;}
      if(own.length===1)headers.Cookie=own[0];
    }
    const upstream=httpRequest(new URL(request.url,origin),{method:request.method,headers,agent},incoming=>{
      const responseHeaders={'Content-Type':incoming.headers['content-type']||'application/json','Cache-Control':'no-store'};
      const cookies=incoming.headers['set-cookie'];
      if(cookies?.length===1) {
        const name=/^([A-Za-z0-9_-]+)=/.exec(cookies[0])?.[1];
        if(!name){incoming.destroy();fail(response);return;}
        cookieName=name;responseHeaders['Set-Cookie']=cookies;
      }
      response.writeHead(incoming.statusCode,responseHeaders);
      incoming.on('error',()=>response.destroy());incoming.pipe(response);
    });
    upstream.setTimeout(15000,()=>upstream.destroy());
    upstream.on('error',()=>fail(response));
    request.once('aborted',()=>upstream.destroy());
    response.once('close',()=>upstream.destroy());
    upstream.end();
  };
}
