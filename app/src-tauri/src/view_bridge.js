// HTML 视图卡页面里的桥（chat_views.rs include_str! 注入每个页面；演示页 visual/ 也读这一份）。改这里两边一起生效。
(function(){
var P=window.parent,q=new URLSearchParams(location.search);
function post(m,p){try{P.postMessage({jsonrpc:"2.0",method:m,params:p},"*")}catch(e){}}
function theme(t){if(t!=="dark"&&t!=="light")return;var d=document.documentElement;d.setAttribute("data-theme",t);d.style.colorScheme=t;window.meowo.theme=t}
function mem(){var s={};return{getItem:function(k){return Object.prototype.hasOwnProperty.call(s,k)?s[k]:null},setItem:function(k,v){s[k]=String(v)},removeItem:function(k){delete s[k]},clear:function(){s={}},key:function(i){return Object.keys(s)[i]||null},get length(){return Object.keys(s).length}}}
["localStorage","sessionStorage"].forEach(function(n){try{window[n].length}catch(e){try{Object.defineProperty(window,n,{value:mem(),configurable:true})}catch(_){}}});
window.meowo={theme:"light",send:function(t){if(typeof t==="string"&&t.trim())post("ui/message",{role:"user",content:{type:"text",text:t}})},openLink:function(u){post("ui/open-link",{url:String(u)})}};
theme(q.get("theme"));
addEventListener("message",function(e){if(e.source!==P)return;var d=e.data;if(d&&d.method==="ui/notifications/host-context-changed"&&d.params)theme(d.params.theme)});
addEventListener("click",function(e){var a=e.target&&e.target.closest&&e.target.closest("a[href]");if(!a)return;var h=a.getAttribute("href")||"";if(h.charAt(0)==="#")return;e.preventDefault();try{var u=new URL(h,"https://invalid.invalid/");if(u.protocol==="http:"||u.protocol==="https:")post("ui/open-link",{url:u.href})}catch(_){}},true);
var last=0;function size(){var b=document.body,d=document.documentElement;var h=Math.ceil(Math.max(d.scrollHeight,b?b.scrollHeight:0));if(h!==last){last=h;post("ui/notifications/size-changed",{height:h})}}
function watch(){size();if(window.ResizeObserver){var r=new ResizeObserver(size);r.observe(document.documentElement);if(document.body)r.observe(document.body)}}
if(document.readyState==="loading")addEventListener("DOMContentLoaded",watch);else watch();addEventListener("load",size);
post("ui/notifications/initialized",{});
})();
