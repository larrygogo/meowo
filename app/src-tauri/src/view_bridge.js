// HTML 视图卡页面里的桥（chat_views.rs include_str! 注入每个页面；演示页 visual/ 也读这一份）。改这里两边一起生效。
(function(){
var P=window.parent,q=new URLSearchParams(location.search);
function post(m,p){try{P.postMessage({jsonrpc:"2.0",method:m,params:p},"*")}catch(e){}}
function theme(t){if(t!=="dark"&&t!=="light")return;var d=document.documentElement;d.setAttribute("data-theme",t);d.style.colorScheme=t;window.meowo.theme=t}
function mem(){var s={};return{getItem:function(k){return Object.prototype.hasOwnProperty.call(s,k)?s[k]:null},setItem:function(k,v){s[k]=String(v)},removeItem:function(k){delete s[k]},clear:function(){s={}},key:function(i){return Object.keys(s)[i]||null},get length(){return Object.keys(s).length}}}
["localStorage","sessionStorage"].forEach(function(n){try{window[n].length}catch(e){try{Object.defineProperty(window,n,{value:mem(),configurable:true})}catch(_){}}});
// 往输入框塞字、开链接只在用户刚点过页面时才发（审查第 1 轮：页面加载就能反复塞字抢焦点、连开浏览器）。
// 宿主那边另有一道：焦点得在这块页面里，缩略图一律不收。
function active(){var u=navigator.userActivation;return !u||u.isActive}
window.meowo={theme:"light",send:function(t){if(typeof t==="string"&&t.trim()&&active())post("ui/message",{role:"user",content:{type:"text",text:t}})},openLink:function(u){if(active())post("ui/open-link",{url:String(u)})}};
theme(q.get("theme"));
addEventListener("message",function(e){if(e.source!==P)return;var d=e.data;if(d&&d.method==="ui/notifications/host-context-changed"&&d.params)theme(d.params.theme)});
addEventListener("click",function(e){var a=e.target&&e.target.closest&&e.target.closest("a[href]");if(!a)return;var h=a.getAttribute("href")||"";if(h.charAt(0)==="#")return;e.preventDefault();try{var u=new URL(h,"https://invalid.invalid/");if((u.protocol==="http:"||u.protocol==="https:")&&active())post("ui/open-link",{url:u.href})}catch(_){}},true);
// 高度量页面内容实际占到哪（body 内容的最低点 + body 下边距），不用 documentElement.scrollHeight：
// 它不会小于 iframe 当前高度，卡片只涨不缩（审查第 1 轮：小页面停在 200px 占位、内容变矮不收）。
// 内容按视口高度排（100vh）时，量出来总比当前视口多一截，卡片一涨页面也跟着涨，会一路撑到上限：
// 连续三次「量出的高度 - 视口高度」不变、视口却在变，就是这种页面，冻住不再报更高的值。
var last=0,gap=null,same=0,frozen=false;
function measure(){var b=document.body;if(!b)return 0;var r=document.createRange();r.selectNodeContents(b);var bottom=r.getBoundingClientRect().bottom;var cs=getComputedStyle(b);return Math.ceil(Math.max(0,bottom+(parseFloat(cs.paddingBottom)||0)+(parseFloat(cs.marginBottom)||0)+(window.scrollY||0)))}
function size(){var h=measure();if(!h||h===last)return;if(h>last){var g=h-window.innerHeight;if(gap!==null&&Math.abs(g-gap)<=1&&g>=0){same++}else{same=0}gap=g;if(same>=2)frozen=true;if(frozen)return}else{same=0;gap=null}last=h;post("ui/notifications/size-changed",{height:h})}
var queued=false;function soon(){if(queued)return;queued=true;requestAnimationFrame(function(){queued=false;size()})}
function watch(){size();if(window.ResizeObserver){var r=new ResizeObserver(soon);r.observe(document.documentElement);if(document.body)r.observe(document.body)}if(window.MutationObserver&&document.body)new MutationObserver(soon).observe(document.body,{subtree:true,childList:true,attributes:true,characterData:true});addEventListener("transitionend",soon,true);addEventListener("animationend",soon,true)}
if(document.readyState==="loading")addEventListener("DOMContentLoaded",watch);else watch();addEventListener("load",size);
post("ui/notifications/initialized",{});
})();
