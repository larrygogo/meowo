var pc = Object.defineProperty;
var mc = (e, t, n) => t in e ? pc(e, t, { enumerable: !0, configurable: !0, writable: !0, value: n }) : e[t] = n;
var nr = (e, t, n) => mc(e, typeof t != "symbol" ? t + "" : t, n);
var Ju = { exports: {} }, rl = {}, qu = { exports: {} }, L = {};
/**
 * @license React
 * react.production.min.js
 *
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
var Gn = Symbol.for("react.element"), hc = Symbol.for("react.portal"), vc = Symbol.for("react.fragment"), yc = Symbol.for("react.strict_mode"), gc = Symbol.for("react.profiler"), wc = Symbol.for("react.provider"), kc = Symbol.for("react.context"), Sc = Symbol.for("react.forward_ref"), xc = Symbol.for("react.suspense"), Ec = Symbol.for("react.memo"), _c = Symbol.for("react.lazy"), $o = Symbol.iterator;
function Cc(e) {
  return e === null || typeof e != "object" ? null : (e = $o && e[$o] || e["@@iterator"], typeof e == "function" ? e : null);
}
var bu = { isMounted: function() {
  return !1;
}, enqueueForceUpdate: function() {
}, enqueueReplaceState: function() {
}, enqueueSetState: function() {
} }, es = Object.assign, ts = {};
function ln(e, t, n) {
  this.props = e, this.context = t, this.refs = ts, this.updater = n || bu;
}
ln.prototype.isReactComponent = {};
ln.prototype.setState = function(e, t) {
  if (typeof e != "object" && typeof e != "function" && e != null) throw Error("setState(...): takes an object of state variables to update or a function which returns an object of state variables.");
  this.updater.enqueueSetState(this, e, t, "setState");
};
ln.prototype.forceUpdate = function(e) {
  this.updater.enqueueForceUpdate(this, e, "forceUpdate");
};
function ns() {
}
ns.prototype = ln.prototype;
function Vi(e, t, n) {
  this.props = e, this.context = t, this.refs = ts, this.updater = n || bu;
}
var Bi = Vi.prototype = new ns();
Bi.constructor = Vi;
es(Bi, ln.prototype);
Bi.isPureReactComponent = !0;
var Uo = Array.isArray, rs = Object.prototype.hasOwnProperty, Hi = { current: null }, ls = { key: !0, ref: !0, __self: !0, __source: !0 };
function is(e, t, n) {
  var r, l = {}, i = null, o = null;
  if (t != null) for (r in t.ref !== void 0 && (o = t.ref), t.key !== void 0 && (i = "" + t.key), t) rs.call(t, r) && !ls.hasOwnProperty(r) && (l[r] = t[r]);
  var u = arguments.length - 2;
  if (u === 1) l.children = n;
  else if (1 < u) {
    for (var s = Array(u), c = 0; c < u; c++) s[c] = arguments[c + 2];
    l.children = s;
  }
  if (e && e.defaultProps) for (r in u = e.defaultProps, u) l[r] === void 0 && (l[r] = u[r]);
  return { $$typeof: Gn, type: e, key: i, ref: o, props: l, _owner: Hi.current };
}
function Nc(e, t) {
  return { $$typeof: Gn, type: e.type, key: t, ref: e.ref, props: e.props, _owner: e._owner };
}
function Wi(e) {
  return typeof e == "object" && e !== null && e.$$typeof === Gn;
}
function zc(e) {
  var t = { "=": "=0", ":": "=2" };
  return "$" + e.replace(/[=:]/g, function(n) {
    return t[n];
  });
}
var Ao = /\/+/g;
function xl(e, t) {
  return typeof e == "object" && e !== null && e.key != null ? zc("" + e.key) : t.toString(36);
}
function Sr(e, t, n, r, l) {
  var i = typeof e;
  (i === "undefined" || i === "boolean") && (e = null);
  var o = !1;
  if (e === null) o = !0;
  else switch (i) {
    case "string":
    case "number":
      o = !0;
      break;
    case "object":
      switch (e.$$typeof) {
        case Gn:
        case hc:
          o = !0;
      }
  }
  if (o) return o = e, l = l(o), e = r === "" ? "." + xl(o, 0) : r, Uo(l) ? (n = "", e != null && (n = e.replace(Ao, "$&/") + "/"), Sr(l, t, n, "", function(c) {
    return c;
  })) : l != null && (Wi(l) && (l = Nc(l, n + (!l.key || o && o.key === l.key ? "" : ("" + l.key).replace(Ao, "$&/") + "/") + e)), t.push(l)), 1;
  if (o = 0, r = r === "" ? "." : r + ":", Uo(e)) for (var u = 0; u < e.length; u++) {
    i = e[u];
    var s = r + xl(i, u);
    o += Sr(i, t, n, s, l);
  }
  else if (s = Cc(e), typeof s == "function") for (e = s.call(e), u = 0; !(i = e.next()).done; ) i = i.value, s = r + xl(i, u++), o += Sr(i, t, n, s, l);
  else if (i === "object") throw t = String(e), Error("Objects are not valid as a React child (found: " + (t === "[object Object]" ? "object with keys {" + Object.keys(e).join(", ") + "}" : t) + "). If you meant to render a collection of children, use an array instead.");
  return o;
}
function rr(e, t, n) {
  if (e == null) return e;
  var r = [], l = 0;
  return Sr(e, r, "", "", function(i) {
    return t.call(n, i, l++);
  }), r;
}
function Pc(e) {
  if (e._status === -1) {
    var t = e._result;
    t = t(), t.then(function(n) {
      (e._status === 0 || e._status === -1) && (e._status = 1, e._result = n);
    }, function(n) {
      (e._status === 0 || e._status === -1) && (e._status = 2, e._result = n);
    }), e._status === -1 && (e._status = 0, e._result = t);
  }
  if (e._status === 1) return e._result.default;
  throw e._result;
}
var se = { current: null }, xr = { transition: null }, Tc = { ReactCurrentDispatcher: se, ReactCurrentBatchConfig: xr, ReactCurrentOwner: Hi };
function os() {
  throw Error("act(...) is not supported in production builds of React.");
}
L.Children = { map: rr, forEach: function(e, t, n) {
  rr(e, function() {
    t.apply(this, arguments);
  }, n);
}, count: function(e) {
  var t = 0;
  return rr(e, function() {
    t++;
  }), t;
}, toArray: function(e) {
  return rr(e, function(t) {
    return t;
  }) || [];
}, only: function(e) {
  if (!Wi(e)) throw Error("React.Children.only expected to receive a single React element child.");
  return e;
} };
L.Component = ln;
L.Fragment = vc;
L.Profiler = gc;
L.PureComponent = Vi;
L.StrictMode = yc;
L.Suspense = xc;
L.__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED = Tc;
L.act = os;
L.cloneElement = function(e, t, n) {
  if (e == null) throw Error("React.cloneElement(...): The argument must be a React element, but you passed " + e + ".");
  var r = es({}, e.props), l = e.key, i = e.ref, o = e._owner;
  if (t != null) {
    if (t.ref !== void 0 && (i = t.ref, o = Hi.current), t.key !== void 0 && (l = "" + t.key), e.type && e.type.defaultProps) var u = e.type.defaultProps;
    for (s in t) rs.call(t, s) && !ls.hasOwnProperty(s) && (r[s] = t[s] === void 0 && u !== void 0 ? u[s] : t[s]);
  }
  var s = arguments.length - 2;
  if (s === 1) r.children = n;
  else if (1 < s) {
    u = Array(s);
    for (var c = 0; c < s; c++) u[c] = arguments[c + 2];
    r.children = u;
  }
  return { $$typeof: Gn, type: e.type, key: l, ref: i, props: r, _owner: o };
};
L.createContext = function(e) {
  return e = { $$typeof: kc, _currentValue: e, _currentValue2: e, _threadCount: 0, Provider: null, Consumer: null, _defaultValue: null, _globalName: null }, e.Provider = { $$typeof: wc, _context: e }, e.Consumer = e;
};
L.createElement = is;
L.createFactory = function(e) {
  var t = is.bind(null, e);
  return t.type = e, t;
};
L.createRef = function() {
  return { current: null };
};
L.forwardRef = function(e) {
  return { $$typeof: Sc, render: e };
};
L.isValidElement = Wi;
L.lazy = function(e) {
  return { $$typeof: _c, _payload: { _status: -1, _result: e }, _init: Pc };
};
L.memo = function(e, t) {
  return { $$typeof: Ec, type: e, compare: t === void 0 ? null : t };
};
L.startTransition = function(e) {
  var t = xr.transition;
  xr.transition = {};
  try {
    e();
  } finally {
    xr.transition = t;
  }
};
L.unstable_act = os;
L.useCallback = function(e, t) {
  return se.current.useCallback(e, t);
};
L.useContext = function(e) {
  return se.current.useContext(e);
};
L.useDebugValue = function() {
};
L.useDeferredValue = function(e) {
  return se.current.useDeferredValue(e);
};
L.useEffect = function(e, t) {
  return se.current.useEffect(e, t);
};
L.useId = function() {
  return se.current.useId();
};
L.useImperativeHandle = function(e, t, n) {
  return se.current.useImperativeHandle(e, t, n);
};
L.useInsertionEffect = function(e, t) {
  return se.current.useInsertionEffect(e, t);
};
L.useLayoutEffect = function(e, t) {
  return se.current.useLayoutEffect(e, t);
};
L.useMemo = function(e, t) {
  return se.current.useMemo(e, t);
};
L.useReducer = function(e, t, n) {
  return se.current.useReducer(e, t, n);
};
L.useRef = function(e) {
  return se.current.useRef(e);
};
L.useState = function(e) {
  return se.current.useState(e);
};
L.useSyncExternalStore = function(e, t, n) {
  return se.current.useSyncExternalStore(e, t, n);
};
L.useTransition = function() {
  return se.current.useTransition();
};
L.version = "18.3.1";
qu.exports = L;
var J = qu.exports;
/**
 * @license React
 * react-jsx-runtime.production.min.js
 *
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
var Lc = J, Mc = Symbol.for("react.element"), Rc = Symbol.for("react.fragment"), jc = Object.prototype.hasOwnProperty, Oc = Lc.__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED.ReactCurrentOwner, Dc = { key: !0, ref: !0, __self: !0, __source: !0 };
function us(e, t, n) {
  var r, l = {}, i = null, o = null;
  n !== void 0 && (i = "" + n), t.key !== void 0 && (i = "" + t.key), t.ref !== void 0 && (o = t.ref);
  for (r in t) jc.call(t, r) && !Dc.hasOwnProperty(r) && (l[r] = t[r]);
  if (e && e.defaultProps) for (r in t = e.defaultProps, t) l[r] === void 0 && (l[r] = t[r]);
  return { $$typeof: Mc, type: e, key: i, ref: o, props: l, _owner: Oc.current };
}
rl.Fragment = Rc;
rl.jsx = us;
rl.jsxs = us;
Ju.exports = rl;
var z = Ju.exports, ss = { exports: {} }, we = {}, as = { exports: {} }, cs = {};
/**
 * @license React
 * scheduler.production.min.js
 *
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
(function(e) {
  function t(E, P) {
    var T = E.length;
    E.push(P);
    e: for (; 0 < T; ) {
      var W = T - 1 >>> 1, G = E[W];
      if (0 < l(G, P)) E[W] = P, E[T] = G, T = W;
      else break e;
    }
  }
  function n(E) {
    return E.length === 0 ? null : E[0];
  }
  function r(E) {
    if (E.length === 0) return null;
    var P = E[0], T = E.pop();
    if (T !== P) {
      E[0] = T;
      e: for (var W = 0, G = E.length, er = G >>> 1; W < er; ) {
        var vt = 2 * (W + 1) - 1, Sl = E[vt], yt = vt + 1, tr = E[yt];
        if (0 > l(Sl, T)) yt < G && 0 > l(tr, Sl) ? (E[W] = tr, E[yt] = T, W = yt) : (E[W] = Sl, E[vt] = T, W = vt);
        else if (yt < G && 0 > l(tr, T)) E[W] = tr, E[yt] = T, W = yt;
        else break e;
      }
    }
    return P;
  }
  function l(E, P) {
    var T = E.sortIndex - P.sortIndex;
    return T !== 0 ? T : E.id - P.id;
  }
  if (typeof performance == "object" && typeof performance.now == "function") {
    var i = performance;
    e.unstable_now = function() {
      return i.now();
    };
  } else {
    var o = Date, u = o.now();
    e.unstable_now = function() {
      return o.now() - u;
    };
  }
  var s = [], c = [], h = 1, m = null, p = 3, g = !1, w = !1, k = !1, O = typeof setTimeout == "function" ? setTimeout : null, d = typeof clearTimeout == "function" ? clearTimeout : null, a = typeof setImmediate < "u" ? setImmediate : null;
  typeof navigator < "u" && navigator.scheduling !== void 0 && navigator.scheduling.isInputPending !== void 0 && navigator.scheduling.isInputPending.bind(navigator.scheduling);
  function f(E) {
    for (var P = n(c); P !== null; ) {
      if (P.callback === null) r(c);
      else if (P.startTime <= E) r(c), P.sortIndex = P.expirationTime, t(s, P);
      else break;
      P = n(c);
    }
  }
  function v(E) {
    if (k = !1, f(E), !w) if (n(s) !== null) w = !0, wl(x);
    else {
      var P = n(c);
      P !== null && kl(v, P.startTime - E);
    }
  }
  function x(E, P) {
    w = !1, k && (k = !1, d(N), N = -1), g = !0;
    var T = p;
    try {
      for (f(P), m = n(s); m !== null && (!(m.expirationTime > P) || E && !ze()); ) {
        var W = m.callback;
        if (typeof W == "function") {
          m.callback = null, p = m.priorityLevel;
          var G = W(m.expirationTime <= P);
          P = e.unstable_now(), typeof G == "function" ? m.callback = G : m === n(s) && r(s), f(P);
        } else r(s);
        m = n(s);
      }
      if (m !== null) var er = !0;
      else {
        var vt = n(c);
        vt !== null && kl(v, vt.startTime - P), er = !1;
      }
      return er;
    } finally {
      m = null, p = T, g = !1;
    }
  }
  var _ = !1, C = null, N = -1, H = 5, M = -1;
  function ze() {
    return !(e.unstable_now() - M < H);
  }
  function sn() {
    if (C !== null) {
      var E = e.unstable_now();
      M = E;
      var P = !0;
      try {
        P = C(!0, E);
      } finally {
        P ? an() : (_ = !1, C = null);
      }
    } else _ = !1;
  }
  var an;
  if (typeof a == "function") an = function() {
    a(sn);
  };
  else if (typeof MessageChannel < "u") {
    var Fo = new MessageChannel(), dc = Fo.port2;
    Fo.port1.onmessage = sn, an = function() {
      dc.postMessage(null);
    };
  } else an = function() {
    O(sn, 0);
  };
  function wl(E) {
    C = E, _ || (_ = !0, an());
  }
  function kl(E, P) {
    N = O(function() {
      E(e.unstable_now());
    }, P);
  }
  e.unstable_IdlePriority = 5, e.unstable_ImmediatePriority = 1, e.unstable_LowPriority = 4, e.unstable_NormalPriority = 3, e.unstable_Profiling = null, e.unstable_UserBlockingPriority = 2, e.unstable_cancelCallback = function(E) {
    E.callback = null;
  }, e.unstable_continueExecution = function() {
    w || g || (w = !0, wl(x));
  }, e.unstable_forceFrameRate = function(E) {
    0 > E || 125 < E ? console.error("forceFrameRate takes a positive int between 0 and 125, forcing frame rates higher than 125 fps is not supported") : H = 0 < E ? Math.floor(1e3 / E) : 5;
  }, e.unstable_getCurrentPriorityLevel = function() {
    return p;
  }, e.unstable_getFirstCallbackNode = function() {
    return n(s);
  }, e.unstable_next = function(E) {
    switch (p) {
      case 1:
      case 2:
      case 3:
        var P = 3;
        break;
      default:
        P = p;
    }
    var T = p;
    p = P;
    try {
      return E();
    } finally {
      p = T;
    }
  }, e.unstable_pauseExecution = function() {
  }, e.unstable_requestPaint = function() {
  }, e.unstable_runWithPriority = function(E, P) {
    switch (E) {
      case 1:
      case 2:
      case 3:
      case 4:
      case 5:
        break;
      default:
        E = 3;
    }
    var T = p;
    p = E;
    try {
      return P();
    } finally {
      p = T;
    }
  }, e.unstable_scheduleCallback = function(E, P, T) {
    var W = e.unstable_now();
    switch (typeof T == "object" && T !== null ? (T = T.delay, T = typeof T == "number" && 0 < T ? W + T : W) : T = W, E) {
      case 1:
        var G = -1;
        break;
      case 2:
        G = 250;
        break;
      case 5:
        G = 1073741823;
        break;
      case 4:
        G = 1e4;
        break;
      default:
        G = 5e3;
    }
    return G = T + G, E = { id: h++, callback: P, priorityLevel: E, startTime: T, expirationTime: G, sortIndex: -1 }, T > W ? (E.sortIndex = T, t(c, E), n(s) === null && E === n(c) && (k ? (d(N), N = -1) : k = !0, kl(v, T - W))) : (E.sortIndex = G, t(s, E), w || g || (w = !0, wl(x))), E;
  }, e.unstable_shouldYield = ze, e.unstable_wrapCallback = function(E) {
    var P = p;
    return function() {
      var T = p;
      p = P;
      try {
        return E.apply(this, arguments);
      } finally {
        p = T;
      }
    };
  };
})(cs);
as.exports = cs;
var Ic = as.exports;
/**
 * @license React
 * react-dom.production.min.js
 *
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
var Fc = J, ge = Ic;
function y(e) {
  for (var t = "https://reactjs.org/docs/error-decoder.html?invariant=" + e, n = 1; n < arguments.length; n++) t += "&args[]=" + encodeURIComponent(arguments[n]);
  return "Minified React error #" + e + "; visit " + t + " for the full message or use the non-minified dev environment for full errors and additional helpful warnings.";
}
var fs = /* @__PURE__ */ new Set(), Rn = {};
function Lt(e, t) {
  Jt(e, t), Jt(e + "Capture", t);
}
function Jt(e, t) {
  for (Rn[e] = t, e = 0; e < t.length; e++) fs.add(t[e]);
}
var Qe = !(typeof window > "u" || typeof window.document > "u" || typeof window.document.createElement > "u"), Gl = Object.prototype.hasOwnProperty, $c = /^[:A-Z_a-z\u00C0-\u00D6\u00D8-\u00F6\u00F8-\u02FF\u0370-\u037D\u037F-\u1FFF\u200C-\u200D\u2070-\u218F\u2C00-\u2FEF\u3001-\uD7FF\uF900-\uFDCF\uFDF0-\uFFFD][:A-Z_a-z\u00C0-\u00D6\u00D8-\u00F6\u00F8-\u02FF\u0370-\u037D\u037F-\u1FFF\u200C-\u200D\u2070-\u218F\u2C00-\u2FEF\u3001-\uD7FF\uF900-\uFDCF\uFDF0-\uFFFD\-.0-9\u00B7\u0300-\u036F\u203F-\u2040]*$/, Vo = {}, Bo = {};
function Uc(e) {
  return Gl.call(Bo, e) ? !0 : Gl.call(Vo, e) ? !1 : $c.test(e) ? Bo[e] = !0 : (Vo[e] = !0, !1);
}
function Ac(e, t, n, r) {
  if (n !== null && n.type === 0) return !1;
  switch (typeof t) {
    case "function":
    case "symbol":
      return !0;
    case "boolean":
      return r ? !1 : n !== null ? !n.acceptsBooleans : (e = e.toLowerCase().slice(0, 5), e !== "data-" && e !== "aria-");
    default:
      return !1;
  }
}
function Vc(e, t, n, r) {
  if (t === null || typeof t > "u" || Ac(e, t, n, r)) return !0;
  if (r) return !1;
  if (n !== null) switch (n.type) {
    case 3:
      return !t;
    case 4:
      return t === !1;
    case 5:
      return isNaN(t);
    case 6:
      return isNaN(t) || 1 > t;
  }
  return !1;
}
function ae(e, t, n, r, l, i, o) {
  this.acceptsBooleans = t === 2 || t === 3 || t === 4, this.attributeName = r, this.attributeNamespace = l, this.mustUseProperty = n, this.propertyName = e, this.type = t, this.sanitizeURL = i, this.removeEmptyString = o;
}
var te = {};
"children dangerouslySetInnerHTML defaultValue defaultChecked innerHTML suppressContentEditableWarning suppressHydrationWarning style".split(" ").forEach(function(e) {
  te[e] = new ae(e, 0, !1, e, null, !1, !1);
});
[["acceptCharset", "accept-charset"], ["className", "class"], ["htmlFor", "for"], ["httpEquiv", "http-equiv"]].forEach(function(e) {
  var t = e[0];
  te[t] = new ae(t, 1, !1, e[1], null, !1, !1);
});
["contentEditable", "draggable", "spellCheck", "value"].forEach(function(e) {
  te[e] = new ae(e, 2, !1, e.toLowerCase(), null, !1, !1);
});
["autoReverse", "externalResourcesRequired", "focusable", "preserveAlpha"].forEach(function(e) {
  te[e] = new ae(e, 2, !1, e, null, !1, !1);
});
"allowFullScreen async autoFocus autoPlay controls default defer disabled disablePictureInPicture disableRemotePlayback formNoValidate hidden loop noModule noValidate open playsInline readOnly required reversed scoped seamless itemScope".split(" ").forEach(function(e) {
  te[e] = new ae(e, 3, !1, e.toLowerCase(), null, !1, !1);
});
["checked", "multiple", "muted", "selected"].forEach(function(e) {
  te[e] = new ae(e, 3, !0, e, null, !1, !1);
});
["capture", "download"].forEach(function(e) {
  te[e] = new ae(e, 4, !1, e, null, !1, !1);
});
["cols", "rows", "size", "span"].forEach(function(e) {
  te[e] = new ae(e, 6, !1, e, null, !1, !1);
});
["rowSpan", "start"].forEach(function(e) {
  te[e] = new ae(e, 5, !1, e.toLowerCase(), null, !1, !1);
});
var Qi = /[\-:]([a-z])/g;
function Yi(e) {
  return e[1].toUpperCase();
}
"accent-height alignment-baseline arabic-form baseline-shift cap-height clip-path clip-rule color-interpolation color-interpolation-filters color-profile color-rendering dominant-baseline enable-background fill-opacity fill-rule flood-color flood-opacity font-family font-size font-size-adjust font-stretch font-style font-variant font-weight glyph-name glyph-orientation-horizontal glyph-orientation-vertical horiz-adv-x horiz-origin-x image-rendering letter-spacing lighting-color marker-end marker-mid marker-start overline-position overline-thickness paint-order panose-1 pointer-events rendering-intent shape-rendering stop-color stop-opacity strikethrough-position strikethrough-thickness stroke-dasharray stroke-dashoffset stroke-linecap stroke-linejoin stroke-miterlimit stroke-opacity stroke-width text-anchor text-decoration text-rendering underline-position underline-thickness unicode-bidi unicode-range units-per-em v-alphabetic v-hanging v-ideographic v-mathematical vector-effect vert-adv-y vert-origin-x vert-origin-y word-spacing writing-mode xmlns:xlink x-height".split(" ").forEach(function(e) {
  var t = e.replace(
    Qi,
    Yi
  );
  te[t] = new ae(t, 1, !1, e, null, !1, !1);
});
"xlink:actuate xlink:arcrole xlink:role xlink:show xlink:title xlink:type".split(" ").forEach(function(e) {
  var t = e.replace(Qi, Yi);
  te[t] = new ae(t, 1, !1, e, "http://www.w3.org/1999/xlink", !1, !1);
});
["xml:base", "xml:lang", "xml:space"].forEach(function(e) {
  var t = e.replace(Qi, Yi);
  te[t] = new ae(t, 1, !1, e, "http://www.w3.org/XML/1998/namespace", !1, !1);
});
["tabIndex", "crossOrigin"].forEach(function(e) {
  te[e] = new ae(e, 1, !1, e.toLowerCase(), null, !1, !1);
});
te.xlinkHref = new ae("xlinkHref", 1, !1, "xlink:href", "http://www.w3.org/1999/xlink", !0, !1);
["src", "href", "action", "formAction"].forEach(function(e) {
  te[e] = new ae(e, 1, !1, e.toLowerCase(), null, !0, !0);
});
function Ki(e, t, n, r) {
  var l = te.hasOwnProperty(t) ? te[t] : null;
  (l !== null ? l.type !== 0 : r || !(2 < t.length) || t[0] !== "o" && t[0] !== "O" || t[1] !== "n" && t[1] !== "N") && (Vc(t, n, l, r) && (n = null), r || l === null ? Uc(t) && (n === null ? e.removeAttribute(t) : e.setAttribute(t, "" + n)) : l.mustUseProperty ? e[l.propertyName] = n === null ? l.type === 3 ? !1 : "" : n : (t = l.attributeName, r = l.attributeNamespace, n === null ? e.removeAttribute(t) : (l = l.type, n = l === 3 || l === 4 && n === !0 ? "" : "" + n, r ? e.setAttributeNS(r, t, n) : e.setAttribute(t, n))));
}
var Ge = Fc.__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED, lr = Symbol.for("react.element"), jt = Symbol.for("react.portal"), Ot = Symbol.for("react.fragment"), Xi = Symbol.for("react.strict_mode"), Zl = Symbol.for("react.profiler"), ds = Symbol.for("react.provider"), ps = Symbol.for("react.context"), Gi = Symbol.for("react.forward_ref"), Jl = Symbol.for("react.suspense"), ql = Symbol.for("react.suspense_list"), Zi = Symbol.for("react.memo"), Je = Symbol.for("react.lazy"), ms = Symbol.for("react.offscreen"), Ho = Symbol.iterator;
function cn(e) {
  return e === null || typeof e != "object" ? null : (e = Ho && e[Ho] || e["@@iterator"], typeof e == "function" ? e : null);
}
var V = Object.assign, El;
function gn(e) {
  if (El === void 0) try {
    throw Error();
  } catch (n) {
    var t = n.stack.trim().match(/\n( *(at )?)/);
    El = t && t[1] || "";
  }
  return `
` + El + e;
}
var _l = !1;
function Cl(e, t) {
  if (!e || _l) return "";
  _l = !0;
  var n = Error.prepareStackTrace;
  Error.prepareStackTrace = void 0;
  try {
    if (t) if (t = function() {
      throw Error();
    }, Object.defineProperty(t.prototype, "props", { set: function() {
      throw Error();
    } }), typeof Reflect == "object" && Reflect.construct) {
      try {
        Reflect.construct(t, []);
      } catch (c) {
        var r = c;
      }
      Reflect.construct(e, [], t);
    } else {
      try {
        t.call();
      } catch (c) {
        r = c;
      }
      e.call(t.prototype);
    }
    else {
      try {
        throw Error();
      } catch (c) {
        r = c;
      }
      e();
    }
  } catch (c) {
    if (c && r && typeof c.stack == "string") {
      for (var l = c.stack.split(`
`), i = r.stack.split(`
`), o = l.length - 1, u = i.length - 1; 1 <= o && 0 <= u && l[o] !== i[u]; ) u--;
      for (; 1 <= o && 0 <= u; o--, u--) if (l[o] !== i[u]) {
        if (o !== 1 || u !== 1)
          do
            if (o--, u--, 0 > u || l[o] !== i[u]) {
              var s = `
` + l[o].replace(" at new ", " at ");
              return e.displayName && s.includes("<anonymous>") && (s = s.replace("<anonymous>", e.displayName)), s;
            }
          while (1 <= o && 0 <= u);
        break;
      }
    }
  } finally {
    _l = !1, Error.prepareStackTrace = n;
  }
  return (e = e ? e.displayName || e.name : "") ? gn(e) : "";
}
function Bc(e) {
  switch (e.tag) {
    case 5:
      return gn(e.type);
    case 16:
      return gn("Lazy");
    case 13:
      return gn("Suspense");
    case 19:
      return gn("SuspenseList");
    case 0:
    case 2:
    case 15:
      return e = Cl(e.type, !1), e;
    case 11:
      return e = Cl(e.type.render, !1), e;
    case 1:
      return e = Cl(e.type, !0), e;
    default:
      return "";
  }
}
function bl(e) {
  if (e == null) return null;
  if (typeof e == "function") return e.displayName || e.name || null;
  if (typeof e == "string") return e;
  switch (e) {
    case Ot:
      return "Fragment";
    case jt:
      return "Portal";
    case Zl:
      return "Profiler";
    case Xi:
      return "StrictMode";
    case Jl:
      return "Suspense";
    case ql:
      return "SuspenseList";
  }
  if (typeof e == "object") switch (e.$$typeof) {
    case ps:
      return (e.displayName || "Context") + ".Consumer";
    case ds:
      return (e._context.displayName || "Context") + ".Provider";
    case Gi:
      var t = e.render;
      return e = e.displayName, e || (e = t.displayName || t.name || "", e = e !== "" ? "ForwardRef(" + e + ")" : "ForwardRef"), e;
    case Zi:
      return t = e.displayName || null, t !== null ? t : bl(e.type) || "Memo";
    case Je:
      t = e._payload, e = e._init;
      try {
        return bl(e(t));
      } catch {
      }
  }
  return null;
}
function Hc(e) {
  var t = e.type;
  switch (e.tag) {
    case 24:
      return "Cache";
    case 9:
      return (t.displayName || "Context") + ".Consumer";
    case 10:
      return (t._context.displayName || "Context") + ".Provider";
    case 18:
      return "DehydratedFragment";
    case 11:
      return e = t.render, e = e.displayName || e.name || "", t.displayName || (e !== "" ? "ForwardRef(" + e + ")" : "ForwardRef");
    case 7:
      return "Fragment";
    case 5:
      return t;
    case 4:
      return "Portal";
    case 3:
      return "Root";
    case 6:
      return "Text";
    case 16:
      return bl(t);
    case 8:
      return t === Xi ? "StrictMode" : "Mode";
    case 22:
      return "Offscreen";
    case 12:
      return "Profiler";
    case 21:
      return "Scope";
    case 13:
      return "Suspense";
    case 19:
      return "SuspenseList";
    case 25:
      return "TracingMarker";
    case 1:
    case 0:
    case 17:
    case 2:
    case 14:
    case 15:
      if (typeof t == "function") return t.displayName || t.name || null;
      if (typeof t == "string") return t;
  }
  return null;
}
function ft(e) {
  switch (typeof e) {
    case "boolean":
    case "number":
    case "string":
    case "undefined":
      return e;
    case "object":
      return e;
    default:
      return "";
  }
}
function hs(e) {
  var t = e.type;
  return (e = e.nodeName) && e.toLowerCase() === "input" && (t === "checkbox" || t === "radio");
}
function Wc(e) {
  var t = hs(e) ? "checked" : "value", n = Object.getOwnPropertyDescriptor(e.constructor.prototype, t), r = "" + e[t];
  if (!e.hasOwnProperty(t) && typeof n < "u" && typeof n.get == "function" && typeof n.set == "function") {
    var l = n.get, i = n.set;
    return Object.defineProperty(e, t, { configurable: !0, get: function() {
      return l.call(this);
    }, set: function(o) {
      r = "" + o, i.call(this, o);
    } }), Object.defineProperty(e, t, { enumerable: n.enumerable }), { getValue: function() {
      return r;
    }, setValue: function(o) {
      r = "" + o;
    }, stopTracking: function() {
      e._valueTracker = null, delete e[t];
    } };
  }
}
function ir(e) {
  e._valueTracker || (e._valueTracker = Wc(e));
}
function vs(e) {
  if (!e) return !1;
  var t = e._valueTracker;
  if (!t) return !0;
  var n = t.getValue(), r = "";
  return e && (r = hs(e) ? e.checked ? "true" : "false" : e.value), e = r, e !== n ? (t.setValue(e), !0) : !1;
}
function jr(e) {
  if (e = e || (typeof document < "u" ? document : void 0), typeof e > "u") return null;
  try {
    return e.activeElement || e.body;
  } catch {
    return e.body;
  }
}
function ei(e, t) {
  var n = t.checked;
  return V({}, t, { defaultChecked: void 0, defaultValue: void 0, value: void 0, checked: n ?? e._wrapperState.initialChecked });
}
function Wo(e, t) {
  var n = t.defaultValue == null ? "" : t.defaultValue, r = t.checked != null ? t.checked : t.defaultChecked;
  n = ft(t.value != null ? t.value : n), e._wrapperState = { initialChecked: r, initialValue: n, controlled: t.type === "checkbox" || t.type === "radio" ? t.checked != null : t.value != null };
}
function ys(e, t) {
  t = t.checked, t != null && Ki(e, "checked", t, !1);
}
function ti(e, t) {
  ys(e, t);
  var n = ft(t.value), r = t.type;
  if (n != null) r === "number" ? (n === 0 && e.value === "" || e.value != n) && (e.value = "" + n) : e.value !== "" + n && (e.value = "" + n);
  else if (r === "submit" || r === "reset") {
    e.removeAttribute("value");
    return;
  }
  t.hasOwnProperty("value") ? ni(e, t.type, n) : t.hasOwnProperty("defaultValue") && ni(e, t.type, ft(t.defaultValue)), t.checked == null && t.defaultChecked != null && (e.defaultChecked = !!t.defaultChecked);
}
function Qo(e, t, n) {
  if (t.hasOwnProperty("value") || t.hasOwnProperty("defaultValue")) {
    var r = t.type;
    if (!(r !== "submit" && r !== "reset" || t.value !== void 0 && t.value !== null)) return;
    t = "" + e._wrapperState.initialValue, n || t === e.value || (e.value = t), e.defaultValue = t;
  }
  n = e.name, n !== "" && (e.name = ""), e.defaultChecked = !!e._wrapperState.initialChecked, n !== "" && (e.name = n);
}
function ni(e, t, n) {
  (t !== "number" || jr(e.ownerDocument) !== e) && (n == null ? e.defaultValue = "" + e._wrapperState.initialValue : e.defaultValue !== "" + n && (e.defaultValue = "" + n));
}
var wn = Array.isArray;
function Qt(e, t, n, r) {
  if (e = e.options, t) {
    t = {};
    for (var l = 0; l < n.length; l++) t["$" + n[l]] = !0;
    for (n = 0; n < e.length; n++) l = t.hasOwnProperty("$" + e[n].value), e[n].selected !== l && (e[n].selected = l), l && r && (e[n].defaultSelected = !0);
  } else {
    for (n = "" + ft(n), t = null, l = 0; l < e.length; l++) {
      if (e[l].value === n) {
        e[l].selected = !0, r && (e[l].defaultSelected = !0);
        return;
      }
      t !== null || e[l].disabled || (t = e[l]);
    }
    t !== null && (t.selected = !0);
  }
}
function ri(e, t) {
  if (t.dangerouslySetInnerHTML != null) throw Error(y(91));
  return V({}, t, { value: void 0, defaultValue: void 0, children: "" + e._wrapperState.initialValue });
}
function Yo(e, t) {
  var n = t.value;
  if (n == null) {
    if (n = t.children, t = t.defaultValue, n != null) {
      if (t != null) throw Error(y(92));
      if (wn(n)) {
        if (1 < n.length) throw Error(y(93));
        n = n[0];
      }
      t = n;
    }
    t == null && (t = ""), n = t;
  }
  e._wrapperState = { initialValue: ft(n) };
}
function gs(e, t) {
  var n = ft(t.value), r = ft(t.defaultValue);
  n != null && (n = "" + n, n !== e.value && (e.value = n), t.defaultValue == null && e.defaultValue !== n && (e.defaultValue = n)), r != null && (e.defaultValue = "" + r);
}
function Ko(e) {
  var t = e.textContent;
  t === e._wrapperState.initialValue && t !== "" && t !== null && (e.value = t);
}
function ws(e) {
  switch (e) {
    case "svg":
      return "http://www.w3.org/2000/svg";
    case "math":
      return "http://www.w3.org/1998/Math/MathML";
    default:
      return "http://www.w3.org/1999/xhtml";
  }
}
function li(e, t) {
  return e == null || e === "http://www.w3.org/1999/xhtml" ? ws(t) : e === "http://www.w3.org/2000/svg" && t === "foreignObject" ? "http://www.w3.org/1999/xhtml" : e;
}
var or, ks = function(e) {
  return typeof MSApp < "u" && MSApp.execUnsafeLocalFunction ? function(t, n, r, l) {
    MSApp.execUnsafeLocalFunction(function() {
      return e(t, n, r, l);
    });
  } : e;
}(function(e, t) {
  if (e.namespaceURI !== "http://www.w3.org/2000/svg" || "innerHTML" in e) e.innerHTML = t;
  else {
    for (or = or || document.createElement("div"), or.innerHTML = "<svg>" + t.valueOf().toString() + "</svg>", t = or.firstChild; e.firstChild; ) e.removeChild(e.firstChild);
    for (; t.firstChild; ) e.appendChild(t.firstChild);
  }
});
function jn(e, t) {
  if (t) {
    var n = e.firstChild;
    if (n && n === e.lastChild && n.nodeType === 3) {
      n.nodeValue = t;
      return;
    }
  }
  e.textContent = t;
}
var xn = {
  animationIterationCount: !0,
  aspectRatio: !0,
  borderImageOutset: !0,
  borderImageSlice: !0,
  borderImageWidth: !0,
  boxFlex: !0,
  boxFlexGroup: !0,
  boxOrdinalGroup: !0,
  columnCount: !0,
  columns: !0,
  flex: !0,
  flexGrow: !0,
  flexPositive: !0,
  flexShrink: !0,
  flexNegative: !0,
  flexOrder: !0,
  gridArea: !0,
  gridRow: !0,
  gridRowEnd: !0,
  gridRowSpan: !0,
  gridRowStart: !0,
  gridColumn: !0,
  gridColumnEnd: !0,
  gridColumnSpan: !0,
  gridColumnStart: !0,
  fontWeight: !0,
  lineClamp: !0,
  lineHeight: !0,
  opacity: !0,
  order: !0,
  orphans: !0,
  tabSize: !0,
  widows: !0,
  zIndex: !0,
  zoom: !0,
  fillOpacity: !0,
  floodOpacity: !0,
  stopOpacity: !0,
  strokeDasharray: !0,
  strokeDashoffset: !0,
  strokeMiterlimit: !0,
  strokeOpacity: !0,
  strokeWidth: !0
}, Qc = ["Webkit", "ms", "Moz", "O"];
Object.keys(xn).forEach(function(e) {
  Qc.forEach(function(t) {
    t = t + e.charAt(0).toUpperCase() + e.substring(1), xn[t] = xn[e];
  });
});
function Ss(e, t, n) {
  return t == null || typeof t == "boolean" || t === "" ? "" : n || typeof t != "number" || t === 0 || xn.hasOwnProperty(e) && xn[e] ? ("" + t).trim() : t + "px";
}
function xs(e, t) {
  e = e.style;
  for (var n in t) if (t.hasOwnProperty(n)) {
    var r = n.indexOf("--") === 0, l = Ss(n, t[n], r);
    n === "float" && (n = "cssFloat"), r ? e.setProperty(n, l) : e[n] = l;
  }
}
var Yc = V({ menuitem: !0 }, { area: !0, base: !0, br: !0, col: !0, embed: !0, hr: !0, img: !0, input: !0, keygen: !0, link: !0, meta: !0, param: !0, source: !0, track: !0, wbr: !0 });
function ii(e, t) {
  if (t) {
    if (Yc[e] && (t.children != null || t.dangerouslySetInnerHTML != null)) throw Error(y(137, e));
    if (t.dangerouslySetInnerHTML != null) {
      if (t.children != null) throw Error(y(60));
      if (typeof t.dangerouslySetInnerHTML != "object" || !("__html" in t.dangerouslySetInnerHTML)) throw Error(y(61));
    }
    if (t.style != null && typeof t.style != "object") throw Error(y(62));
  }
}
function oi(e, t) {
  if (e.indexOf("-") === -1) return typeof t.is == "string";
  switch (e) {
    case "annotation-xml":
    case "color-profile":
    case "font-face":
    case "font-face-src":
    case "font-face-uri":
    case "font-face-format":
    case "font-face-name":
    case "missing-glyph":
      return !1;
    default:
      return !0;
  }
}
var ui = null;
function Ji(e) {
  return e = e.target || e.srcElement || window, e.correspondingUseElement && (e = e.correspondingUseElement), e.nodeType === 3 ? e.parentNode : e;
}
var si = null, Yt = null, Kt = null;
function Xo(e) {
  if (e = qn(e)) {
    if (typeof si != "function") throw Error(y(280));
    var t = e.stateNode;
    t && (t = sl(t), si(e.stateNode, e.type, t));
  }
}
function Es(e) {
  Yt ? Kt ? Kt.push(e) : Kt = [e] : Yt = e;
}
function _s() {
  if (Yt) {
    var e = Yt, t = Kt;
    if (Kt = Yt = null, Xo(e), t) for (e = 0; e < t.length; e++) Xo(t[e]);
  }
}
function Cs(e, t) {
  return e(t);
}
function Ns() {
}
var Nl = !1;
function zs(e, t, n) {
  if (Nl) return e(t, n);
  Nl = !0;
  try {
    return Cs(e, t, n);
  } finally {
    Nl = !1, (Yt !== null || Kt !== null) && (Ns(), _s());
  }
}
function On(e, t) {
  var n = e.stateNode;
  if (n === null) return null;
  var r = sl(n);
  if (r === null) return null;
  n = r[t];
  e: switch (t) {
    case "onClick":
    case "onClickCapture":
    case "onDoubleClick":
    case "onDoubleClickCapture":
    case "onMouseDown":
    case "onMouseDownCapture":
    case "onMouseMove":
    case "onMouseMoveCapture":
    case "onMouseUp":
    case "onMouseUpCapture":
    case "onMouseEnter":
      (r = !r.disabled) || (e = e.type, r = !(e === "button" || e === "input" || e === "select" || e === "textarea")), e = !r;
      break e;
    default:
      e = !1;
  }
  if (e) return null;
  if (n && typeof n != "function") throw Error(y(231, t, typeof n));
  return n;
}
var ai = !1;
if (Qe) try {
  var fn = {};
  Object.defineProperty(fn, "passive", { get: function() {
    ai = !0;
  } }), window.addEventListener("test", fn, fn), window.removeEventListener("test", fn, fn);
} catch {
  ai = !1;
}
function Kc(e, t, n, r, l, i, o, u, s) {
  var c = Array.prototype.slice.call(arguments, 3);
  try {
    t.apply(n, c);
  } catch (h) {
    this.onError(h);
  }
}
var En = !1, Or = null, Dr = !1, ci = null, Xc = { onError: function(e) {
  En = !0, Or = e;
} };
function Gc(e, t, n, r, l, i, o, u, s) {
  En = !1, Or = null, Kc.apply(Xc, arguments);
}
function Zc(e, t, n, r, l, i, o, u, s) {
  if (Gc.apply(this, arguments), En) {
    if (En) {
      var c = Or;
      En = !1, Or = null;
    } else throw Error(y(198));
    Dr || (Dr = !0, ci = c);
  }
}
function Mt(e) {
  var t = e, n = e;
  if (e.alternate) for (; t.return; ) t = t.return;
  else {
    e = t;
    do
      t = e, t.flags & 4098 && (n = t.return), e = t.return;
    while (e);
  }
  return t.tag === 3 ? n : null;
}
function Ps(e) {
  if (e.tag === 13) {
    var t = e.memoizedState;
    if (t === null && (e = e.alternate, e !== null && (t = e.memoizedState)), t !== null) return t.dehydrated;
  }
  return null;
}
function Go(e) {
  if (Mt(e) !== e) throw Error(y(188));
}
function Jc(e) {
  var t = e.alternate;
  if (!t) {
    if (t = Mt(e), t === null) throw Error(y(188));
    return t !== e ? null : e;
  }
  for (var n = e, r = t; ; ) {
    var l = n.return;
    if (l === null) break;
    var i = l.alternate;
    if (i === null) {
      if (r = l.return, r !== null) {
        n = r;
        continue;
      }
      break;
    }
    if (l.child === i.child) {
      for (i = l.child; i; ) {
        if (i === n) return Go(l), e;
        if (i === r) return Go(l), t;
        i = i.sibling;
      }
      throw Error(y(188));
    }
    if (n.return !== r.return) n = l, r = i;
    else {
      for (var o = !1, u = l.child; u; ) {
        if (u === n) {
          o = !0, n = l, r = i;
          break;
        }
        if (u === r) {
          o = !0, r = l, n = i;
          break;
        }
        u = u.sibling;
      }
      if (!o) {
        for (u = i.child; u; ) {
          if (u === n) {
            o = !0, n = i, r = l;
            break;
          }
          if (u === r) {
            o = !0, r = i, n = l;
            break;
          }
          u = u.sibling;
        }
        if (!o) throw Error(y(189));
      }
    }
    if (n.alternate !== r) throw Error(y(190));
  }
  if (n.tag !== 3) throw Error(y(188));
  return n.stateNode.current === n ? e : t;
}
function Ts(e) {
  return e = Jc(e), e !== null ? Ls(e) : null;
}
function Ls(e) {
  if (e.tag === 5 || e.tag === 6) return e;
  for (e = e.child; e !== null; ) {
    var t = Ls(e);
    if (t !== null) return t;
    e = e.sibling;
  }
  return null;
}
var Ms = ge.unstable_scheduleCallback, Zo = ge.unstable_cancelCallback, qc = ge.unstable_shouldYield, bc = ge.unstable_requestPaint, Q = ge.unstable_now, ef = ge.unstable_getCurrentPriorityLevel, qi = ge.unstable_ImmediatePriority, Rs = ge.unstable_UserBlockingPriority, Ir = ge.unstable_NormalPriority, tf = ge.unstable_LowPriority, js = ge.unstable_IdlePriority, ll = null, $e = null;
function nf(e) {
  if ($e && typeof $e.onCommitFiberRoot == "function") try {
    $e.onCommitFiberRoot(ll, e, void 0, (e.current.flags & 128) === 128);
  } catch {
  }
}
var Re = Math.clz32 ? Math.clz32 : of, rf = Math.log, lf = Math.LN2;
function of(e) {
  return e >>>= 0, e === 0 ? 32 : 31 - (rf(e) / lf | 0) | 0;
}
var ur = 64, sr = 4194304;
function kn(e) {
  switch (e & -e) {
    case 1:
      return 1;
    case 2:
      return 2;
    case 4:
      return 4;
    case 8:
      return 8;
    case 16:
      return 16;
    case 32:
      return 32;
    case 64:
    case 128:
    case 256:
    case 512:
    case 1024:
    case 2048:
    case 4096:
    case 8192:
    case 16384:
    case 32768:
    case 65536:
    case 131072:
    case 262144:
    case 524288:
    case 1048576:
    case 2097152:
      return e & 4194240;
    case 4194304:
    case 8388608:
    case 16777216:
    case 33554432:
    case 67108864:
      return e & 130023424;
    case 134217728:
      return 134217728;
    case 268435456:
      return 268435456;
    case 536870912:
      return 536870912;
    case 1073741824:
      return 1073741824;
    default:
      return e;
  }
}
function Fr(e, t) {
  var n = e.pendingLanes;
  if (n === 0) return 0;
  var r = 0, l = e.suspendedLanes, i = e.pingedLanes, o = n & 268435455;
  if (o !== 0) {
    var u = o & ~l;
    u !== 0 ? r = kn(u) : (i &= o, i !== 0 && (r = kn(i)));
  } else o = n & ~l, o !== 0 ? r = kn(o) : i !== 0 && (r = kn(i));
  if (r === 0) return 0;
  if (t !== 0 && t !== r && !(t & l) && (l = r & -r, i = t & -t, l >= i || l === 16 && (i & 4194240) !== 0)) return t;
  if (r & 4 && (r |= n & 16), t = e.entangledLanes, t !== 0) for (e = e.entanglements, t &= r; 0 < t; ) n = 31 - Re(t), l = 1 << n, r |= e[n], t &= ~l;
  return r;
}
function uf(e, t) {
  switch (e) {
    case 1:
    case 2:
    case 4:
      return t + 250;
    case 8:
    case 16:
    case 32:
    case 64:
    case 128:
    case 256:
    case 512:
    case 1024:
    case 2048:
    case 4096:
    case 8192:
    case 16384:
    case 32768:
    case 65536:
    case 131072:
    case 262144:
    case 524288:
    case 1048576:
    case 2097152:
      return t + 5e3;
    case 4194304:
    case 8388608:
    case 16777216:
    case 33554432:
    case 67108864:
      return -1;
    case 134217728:
    case 268435456:
    case 536870912:
    case 1073741824:
      return -1;
    default:
      return -1;
  }
}
function sf(e, t) {
  for (var n = e.suspendedLanes, r = e.pingedLanes, l = e.expirationTimes, i = e.pendingLanes; 0 < i; ) {
    var o = 31 - Re(i), u = 1 << o, s = l[o];
    s === -1 ? (!(u & n) || u & r) && (l[o] = uf(u, t)) : s <= t && (e.expiredLanes |= u), i &= ~u;
  }
}
function fi(e) {
  return e = e.pendingLanes & -1073741825, e !== 0 ? e : e & 1073741824 ? 1073741824 : 0;
}
function Os() {
  var e = ur;
  return ur <<= 1, !(ur & 4194240) && (ur = 64), e;
}
function zl(e) {
  for (var t = [], n = 0; 31 > n; n++) t.push(e);
  return t;
}
function Zn(e, t, n) {
  e.pendingLanes |= t, t !== 536870912 && (e.suspendedLanes = 0, e.pingedLanes = 0), e = e.eventTimes, t = 31 - Re(t), e[t] = n;
}
function af(e, t) {
  var n = e.pendingLanes & ~t;
  e.pendingLanes = t, e.suspendedLanes = 0, e.pingedLanes = 0, e.expiredLanes &= t, e.mutableReadLanes &= t, e.entangledLanes &= t, t = e.entanglements;
  var r = e.eventTimes;
  for (e = e.expirationTimes; 0 < n; ) {
    var l = 31 - Re(n), i = 1 << l;
    t[l] = 0, r[l] = -1, e[l] = -1, n &= ~i;
  }
}
function bi(e, t) {
  var n = e.entangledLanes |= t;
  for (e = e.entanglements; n; ) {
    var r = 31 - Re(n), l = 1 << r;
    l & t | e[r] & t && (e[r] |= t), n &= ~l;
  }
}
var j = 0;
function Ds(e) {
  return e &= -e, 1 < e ? 4 < e ? e & 268435455 ? 16 : 536870912 : 4 : 1;
}
var Is, eo, Fs, $s, Us, di = !1, ar = [], rt = null, lt = null, it = null, Dn = /* @__PURE__ */ new Map(), In = /* @__PURE__ */ new Map(), be = [], cf = "mousedown mouseup touchcancel touchend touchstart auxclick dblclick pointercancel pointerdown pointerup dragend dragstart drop compositionend compositionstart keydown keypress keyup input textInput copy cut paste click change contextmenu reset submit".split(" ");
function Jo(e, t) {
  switch (e) {
    case "focusin":
    case "focusout":
      rt = null;
      break;
    case "dragenter":
    case "dragleave":
      lt = null;
      break;
    case "mouseover":
    case "mouseout":
      it = null;
      break;
    case "pointerover":
    case "pointerout":
      Dn.delete(t.pointerId);
      break;
    case "gotpointercapture":
    case "lostpointercapture":
      In.delete(t.pointerId);
  }
}
function dn(e, t, n, r, l, i) {
  return e === null || e.nativeEvent !== i ? (e = { blockedOn: t, domEventName: n, eventSystemFlags: r, nativeEvent: i, targetContainers: [l] }, t !== null && (t = qn(t), t !== null && eo(t)), e) : (e.eventSystemFlags |= r, t = e.targetContainers, l !== null && t.indexOf(l) === -1 && t.push(l), e);
}
function ff(e, t, n, r, l) {
  switch (t) {
    case "focusin":
      return rt = dn(rt, e, t, n, r, l), !0;
    case "dragenter":
      return lt = dn(lt, e, t, n, r, l), !0;
    case "mouseover":
      return it = dn(it, e, t, n, r, l), !0;
    case "pointerover":
      var i = l.pointerId;
      return Dn.set(i, dn(Dn.get(i) || null, e, t, n, r, l)), !0;
    case "gotpointercapture":
      return i = l.pointerId, In.set(i, dn(In.get(i) || null, e, t, n, r, l)), !0;
  }
  return !1;
}
function As(e) {
  var t = kt(e.target);
  if (t !== null) {
    var n = Mt(t);
    if (n !== null) {
      if (t = n.tag, t === 13) {
        if (t = Ps(n), t !== null) {
          e.blockedOn = t, Us(e.priority, function() {
            Fs(n);
          });
          return;
        }
      } else if (t === 3 && n.stateNode.current.memoizedState.isDehydrated) {
        e.blockedOn = n.tag === 3 ? n.stateNode.containerInfo : null;
        return;
      }
    }
  }
  e.blockedOn = null;
}
function Er(e) {
  if (e.blockedOn !== null) return !1;
  for (var t = e.targetContainers; 0 < t.length; ) {
    var n = pi(e.domEventName, e.eventSystemFlags, t[0], e.nativeEvent);
    if (n === null) {
      n = e.nativeEvent;
      var r = new n.constructor(n.type, n);
      ui = r, n.target.dispatchEvent(r), ui = null;
    } else return t = qn(n), t !== null && eo(t), e.blockedOn = n, !1;
    t.shift();
  }
  return !0;
}
function qo(e, t, n) {
  Er(e) && n.delete(t);
}
function df() {
  di = !1, rt !== null && Er(rt) && (rt = null), lt !== null && Er(lt) && (lt = null), it !== null && Er(it) && (it = null), Dn.forEach(qo), In.forEach(qo);
}
function pn(e, t) {
  e.blockedOn === t && (e.blockedOn = null, di || (di = !0, ge.unstable_scheduleCallback(ge.unstable_NormalPriority, df)));
}
function Fn(e) {
  function t(l) {
    return pn(l, e);
  }
  if (0 < ar.length) {
    pn(ar[0], e);
    for (var n = 1; n < ar.length; n++) {
      var r = ar[n];
      r.blockedOn === e && (r.blockedOn = null);
    }
  }
  for (rt !== null && pn(rt, e), lt !== null && pn(lt, e), it !== null && pn(it, e), Dn.forEach(t), In.forEach(t), n = 0; n < be.length; n++) r = be[n], r.blockedOn === e && (r.blockedOn = null);
  for (; 0 < be.length && (n = be[0], n.blockedOn === null); ) As(n), n.blockedOn === null && be.shift();
}
var Xt = Ge.ReactCurrentBatchConfig, $r = !0;
function pf(e, t, n, r) {
  var l = j, i = Xt.transition;
  Xt.transition = null;
  try {
    j = 1, to(e, t, n, r);
  } finally {
    j = l, Xt.transition = i;
  }
}
function mf(e, t, n, r) {
  var l = j, i = Xt.transition;
  Xt.transition = null;
  try {
    j = 4, to(e, t, n, r);
  } finally {
    j = l, Xt.transition = i;
  }
}
function to(e, t, n, r) {
  if ($r) {
    var l = pi(e, t, n, r);
    if (l === null) Fl(e, t, r, Ur, n), Jo(e, r);
    else if (ff(l, e, t, n, r)) r.stopPropagation();
    else if (Jo(e, r), t & 4 && -1 < cf.indexOf(e)) {
      for (; l !== null; ) {
        var i = qn(l);
        if (i !== null && Is(i), i = pi(e, t, n, r), i === null && Fl(e, t, r, Ur, n), i === l) break;
        l = i;
      }
      l !== null && r.stopPropagation();
    } else Fl(e, t, r, null, n);
  }
}
var Ur = null;
function pi(e, t, n, r) {
  if (Ur = null, e = Ji(r), e = kt(e), e !== null) if (t = Mt(e), t === null) e = null;
  else if (n = t.tag, n === 13) {
    if (e = Ps(t), e !== null) return e;
    e = null;
  } else if (n === 3) {
    if (t.stateNode.current.memoizedState.isDehydrated) return t.tag === 3 ? t.stateNode.containerInfo : null;
    e = null;
  } else t !== e && (e = null);
  return Ur = e, null;
}
function Vs(e) {
  switch (e) {
    case "cancel":
    case "click":
    case "close":
    case "contextmenu":
    case "copy":
    case "cut":
    case "auxclick":
    case "dblclick":
    case "dragend":
    case "dragstart":
    case "drop":
    case "focusin":
    case "focusout":
    case "input":
    case "invalid":
    case "keydown":
    case "keypress":
    case "keyup":
    case "mousedown":
    case "mouseup":
    case "paste":
    case "pause":
    case "play":
    case "pointercancel":
    case "pointerdown":
    case "pointerup":
    case "ratechange":
    case "reset":
    case "resize":
    case "seeked":
    case "submit":
    case "touchcancel":
    case "touchend":
    case "touchstart":
    case "volumechange":
    case "change":
    case "selectionchange":
    case "textInput":
    case "compositionstart":
    case "compositionend":
    case "compositionupdate":
    case "beforeblur":
    case "afterblur":
    case "beforeinput":
    case "blur":
    case "fullscreenchange":
    case "focus":
    case "hashchange":
    case "popstate":
    case "select":
    case "selectstart":
      return 1;
    case "drag":
    case "dragenter":
    case "dragexit":
    case "dragleave":
    case "dragover":
    case "mousemove":
    case "mouseout":
    case "mouseover":
    case "pointermove":
    case "pointerout":
    case "pointerover":
    case "scroll":
    case "toggle":
    case "touchmove":
    case "wheel":
    case "mouseenter":
    case "mouseleave":
    case "pointerenter":
    case "pointerleave":
      return 4;
    case "message":
      switch (ef()) {
        case qi:
          return 1;
        case Rs:
          return 4;
        case Ir:
        case tf:
          return 16;
        case js:
          return 536870912;
        default:
          return 16;
      }
    default:
      return 16;
  }
}
var tt = null, no = null, _r = null;
function Bs() {
  if (_r) return _r;
  var e, t = no, n = t.length, r, l = "value" in tt ? tt.value : tt.textContent, i = l.length;
  for (e = 0; e < n && t[e] === l[e]; e++) ;
  var o = n - e;
  for (r = 1; r <= o && t[n - r] === l[i - r]; r++) ;
  return _r = l.slice(e, 1 < r ? 1 - r : void 0);
}
function Cr(e) {
  var t = e.keyCode;
  return "charCode" in e ? (e = e.charCode, e === 0 && t === 13 && (e = 13)) : e = t, e === 10 && (e = 13), 32 <= e || e === 13 ? e : 0;
}
function cr() {
  return !0;
}
function bo() {
  return !1;
}
function ke(e) {
  function t(n, r, l, i, o) {
    this._reactName = n, this._targetInst = l, this.type = r, this.nativeEvent = i, this.target = o, this.currentTarget = null;
    for (var u in e) e.hasOwnProperty(u) && (n = e[u], this[u] = n ? n(i) : i[u]);
    return this.isDefaultPrevented = (i.defaultPrevented != null ? i.defaultPrevented : i.returnValue === !1) ? cr : bo, this.isPropagationStopped = bo, this;
  }
  return V(t.prototype, { preventDefault: function() {
    this.defaultPrevented = !0;
    var n = this.nativeEvent;
    n && (n.preventDefault ? n.preventDefault() : typeof n.returnValue != "unknown" && (n.returnValue = !1), this.isDefaultPrevented = cr);
  }, stopPropagation: function() {
    var n = this.nativeEvent;
    n && (n.stopPropagation ? n.stopPropagation() : typeof n.cancelBubble != "unknown" && (n.cancelBubble = !0), this.isPropagationStopped = cr);
  }, persist: function() {
  }, isPersistent: cr }), t;
}
var on = { eventPhase: 0, bubbles: 0, cancelable: 0, timeStamp: function(e) {
  return e.timeStamp || Date.now();
}, defaultPrevented: 0, isTrusted: 0 }, ro = ke(on), Jn = V({}, on, { view: 0, detail: 0 }), hf = ke(Jn), Pl, Tl, mn, il = V({}, Jn, { screenX: 0, screenY: 0, clientX: 0, clientY: 0, pageX: 0, pageY: 0, ctrlKey: 0, shiftKey: 0, altKey: 0, metaKey: 0, getModifierState: lo, button: 0, buttons: 0, relatedTarget: function(e) {
  return e.relatedTarget === void 0 ? e.fromElement === e.srcElement ? e.toElement : e.fromElement : e.relatedTarget;
}, movementX: function(e) {
  return "movementX" in e ? e.movementX : (e !== mn && (mn && e.type === "mousemove" ? (Pl = e.screenX - mn.screenX, Tl = e.screenY - mn.screenY) : Tl = Pl = 0, mn = e), Pl);
}, movementY: function(e) {
  return "movementY" in e ? e.movementY : Tl;
} }), eu = ke(il), vf = V({}, il, { dataTransfer: 0 }), yf = ke(vf), gf = V({}, Jn, { relatedTarget: 0 }), Ll = ke(gf), wf = V({}, on, { animationName: 0, elapsedTime: 0, pseudoElement: 0 }), kf = ke(wf), Sf = V({}, on, { clipboardData: function(e) {
  return "clipboardData" in e ? e.clipboardData : window.clipboardData;
} }), xf = ke(Sf), Ef = V({}, on, { data: 0 }), tu = ke(Ef), _f = {
  Esc: "Escape",
  Spacebar: " ",
  Left: "ArrowLeft",
  Up: "ArrowUp",
  Right: "ArrowRight",
  Down: "ArrowDown",
  Del: "Delete",
  Win: "OS",
  Menu: "ContextMenu",
  Apps: "ContextMenu",
  Scroll: "ScrollLock",
  MozPrintableKey: "Unidentified"
}, Cf = {
  8: "Backspace",
  9: "Tab",
  12: "Clear",
  13: "Enter",
  16: "Shift",
  17: "Control",
  18: "Alt",
  19: "Pause",
  20: "CapsLock",
  27: "Escape",
  32: " ",
  33: "PageUp",
  34: "PageDown",
  35: "End",
  36: "Home",
  37: "ArrowLeft",
  38: "ArrowUp",
  39: "ArrowRight",
  40: "ArrowDown",
  45: "Insert",
  46: "Delete",
  112: "F1",
  113: "F2",
  114: "F3",
  115: "F4",
  116: "F5",
  117: "F6",
  118: "F7",
  119: "F8",
  120: "F9",
  121: "F10",
  122: "F11",
  123: "F12",
  144: "NumLock",
  145: "ScrollLock",
  224: "Meta"
}, Nf = { Alt: "altKey", Control: "ctrlKey", Meta: "metaKey", Shift: "shiftKey" };
function zf(e) {
  var t = this.nativeEvent;
  return t.getModifierState ? t.getModifierState(e) : (e = Nf[e]) ? !!t[e] : !1;
}
function lo() {
  return zf;
}
var Pf = V({}, Jn, { key: function(e) {
  if (e.key) {
    var t = _f[e.key] || e.key;
    if (t !== "Unidentified") return t;
  }
  return e.type === "keypress" ? (e = Cr(e), e === 13 ? "Enter" : String.fromCharCode(e)) : e.type === "keydown" || e.type === "keyup" ? Cf[e.keyCode] || "Unidentified" : "";
}, code: 0, location: 0, ctrlKey: 0, shiftKey: 0, altKey: 0, metaKey: 0, repeat: 0, locale: 0, getModifierState: lo, charCode: function(e) {
  return e.type === "keypress" ? Cr(e) : 0;
}, keyCode: function(e) {
  return e.type === "keydown" || e.type === "keyup" ? e.keyCode : 0;
}, which: function(e) {
  return e.type === "keypress" ? Cr(e) : e.type === "keydown" || e.type === "keyup" ? e.keyCode : 0;
} }), Tf = ke(Pf), Lf = V({}, il, { pointerId: 0, width: 0, height: 0, pressure: 0, tangentialPressure: 0, tiltX: 0, tiltY: 0, twist: 0, pointerType: 0, isPrimary: 0 }), nu = ke(Lf), Mf = V({}, Jn, { touches: 0, targetTouches: 0, changedTouches: 0, altKey: 0, metaKey: 0, ctrlKey: 0, shiftKey: 0, getModifierState: lo }), Rf = ke(Mf), jf = V({}, on, { propertyName: 0, elapsedTime: 0, pseudoElement: 0 }), Of = ke(jf), Df = V({}, il, {
  deltaX: function(e) {
    return "deltaX" in e ? e.deltaX : "wheelDeltaX" in e ? -e.wheelDeltaX : 0;
  },
  deltaY: function(e) {
    return "deltaY" in e ? e.deltaY : "wheelDeltaY" in e ? -e.wheelDeltaY : "wheelDelta" in e ? -e.wheelDelta : 0;
  },
  deltaZ: 0,
  deltaMode: 0
}), If = ke(Df), Ff = [9, 13, 27, 32], io = Qe && "CompositionEvent" in window, _n = null;
Qe && "documentMode" in document && (_n = document.documentMode);
var $f = Qe && "TextEvent" in window && !_n, Hs = Qe && (!io || _n && 8 < _n && 11 >= _n), ru = " ", lu = !1;
function Ws(e, t) {
  switch (e) {
    case "keyup":
      return Ff.indexOf(t.keyCode) !== -1;
    case "keydown":
      return t.keyCode !== 229;
    case "keypress":
    case "mousedown":
    case "focusout":
      return !0;
    default:
      return !1;
  }
}
function Qs(e) {
  return e = e.detail, typeof e == "object" && "data" in e ? e.data : null;
}
var Dt = !1;
function Uf(e, t) {
  switch (e) {
    case "compositionend":
      return Qs(t);
    case "keypress":
      return t.which !== 32 ? null : (lu = !0, ru);
    case "textInput":
      return e = t.data, e === ru && lu ? null : e;
    default:
      return null;
  }
}
function Af(e, t) {
  if (Dt) return e === "compositionend" || !io && Ws(e, t) ? (e = Bs(), _r = no = tt = null, Dt = !1, e) : null;
  switch (e) {
    case "paste":
      return null;
    case "keypress":
      if (!(t.ctrlKey || t.altKey || t.metaKey) || t.ctrlKey && t.altKey) {
        if (t.char && 1 < t.char.length) return t.char;
        if (t.which) return String.fromCharCode(t.which);
      }
      return null;
    case "compositionend":
      return Hs && t.locale !== "ko" ? null : t.data;
    default:
      return null;
  }
}
var Vf = { color: !0, date: !0, datetime: !0, "datetime-local": !0, email: !0, month: !0, number: !0, password: !0, range: !0, search: !0, tel: !0, text: !0, time: !0, url: !0, week: !0 };
function iu(e) {
  var t = e && e.nodeName && e.nodeName.toLowerCase();
  return t === "input" ? !!Vf[e.type] : t === "textarea";
}
function Ys(e, t, n, r) {
  Es(r), t = Ar(t, "onChange"), 0 < t.length && (n = new ro("onChange", "change", null, n, r), e.push({ event: n, listeners: t }));
}
var Cn = null, $n = null;
function Bf(e) {
  ra(e, 0);
}
function ol(e) {
  var t = $t(e);
  if (vs(t)) return e;
}
function Hf(e, t) {
  if (e === "change") return t;
}
var Ks = !1;
if (Qe) {
  var Ml;
  if (Qe) {
    var Rl = "oninput" in document;
    if (!Rl) {
      var ou = document.createElement("div");
      ou.setAttribute("oninput", "return;"), Rl = typeof ou.oninput == "function";
    }
    Ml = Rl;
  } else Ml = !1;
  Ks = Ml && (!document.documentMode || 9 < document.documentMode);
}
function uu() {
  Cn && (Cn.detachEvent("onpropertychange", Xs), $n = Cn = null);
}
function Xs(e) {
  if (e.propertyName === "value" && ol($n)) {
    var t = [];
    Ys(t, $n, e, Ji(e)), zs(Bf, t);
  }
}
function Wf(e, t, n) {
  e === "focusin" ? (uu(), Cn = t, $n = n, Cn.attachEvent("onpropertychange", Xs)) : e === "focusout" && uu();
}
function Qf(e) {
  if (e === "selectionchange" || e === "keyup" || e === "keydown") return ol($n);
}
function Yf(e, t) {
  if (e === "click") return ol(t);
}
function Kf(e, t) {
  if (e === "input" || e === "change") return ol(t);
}
function Xf(e, t) {
  return e === t && (e !== 0 || 1 / e === 1 / t) || e !== e && t !== t;
}
var Oe = typeof Object.is == "function" ? Object.is : Xf;
function Un(e, t) {
  if (Oe(e, t)) return !0;
  if (typeof e != "object" || e === null || typeof t != "object" || t === null) return !1;
  var n = Object.keys(e), r = Object.keys(t);
  if (n.length !== r.length) return !1;
  for (r = 0; r < n.length; r++) {
    var l = n[r];
    if (!Gl.call(t, l) || !Oe(e[l], t[l])) return !1;
  }
  return !0;
}
function su(e) {
  for (; e && e.firstChild; ) e = e.firstChild;
  return e;
}
function au(e, t) {
  var n = su(e);
  e = 0;
  for (var r; n; ) {
    if (n.nodeType === 3) {
      if (r = e + n.textContent.length, e <= t && r >= t) return { node: n, offset: t - e };
      e = r;
    }
    e: {
      for (; n; ) {
        if (n.nextSibling) {
          n = n.nextSibling;
          break e;
        }
        n = n.parentNode;
      }
      n = void 0;
    }
    n = su(n);
  }
}
function Gs(e, t) {
  return e && t ? e === t ? !0 : e && e.nodeType === 3 ? !1 : t && t.nodeType === 3 ? Gs(e, t.parentNode) : "contains" in e ? e.contains(t) : e.compareDocumentPosition ? !!(e.compareDocumentPosition(t) & 16) : !1 : !1;
}
function Zs() {
  for (var e = window, t = jr(); t instanceof e.HTMLIFrameElement; ) {
    try {
      var n = typeof t.contentWindow.location.href == "string";
    } catch {
      n = !1;
    }
    if (n) e = t.contentWindow;
    else break;
    t = jr(e.document);
  }
  return t;
}
function oo(e) {
  var t = e && e.nodeName && e.nodeName.toLowerCase();
  return t && (t === "input" && (e.type === "text" || e.type === "search" || e.type === "tel" || e.type === "url" || e.type === "password") || t === "textarea" || e.contentEditable === "true");
}
function Gf(e) {
  var t = Zs(), n = e.focusedElem, r = e.selectionRange;
  if (t !== n && n && n.ownerDocument && Gs(n.ownerDocument.documentElement, n)) {
    if (r !== null && oo(n)) {
      if (t = r.start, e = r.end, e === void 0 && (e = t), "selectionStart" in n) n.selectionStart = t, n.selectionEnd = Math.min(e, n.value.length);
      else if (e = (t = n.ownerDocument || document) && t.defaultView || window, e.getSelection) {
        e = e.getSelection();
        var l = n.textContent.length, i = Math.min(r.start, l);
        r = r.end === void 0 ? i : Math.min(r.end, l), !e.extend && i > r && (l = r, r = i, i = l), l = au(n, i);
        var o = au(
          n,
          r
        );
        l && o && (e.rangeCount !== 1 || e.anchorNode !== l.node || e.anchorOffset !== l.offset || e.focusNode !== o.node || e.focusOffset !== o.offset) && (t = t.createRange(), t.setStart(l.node, l.offset), e.removeAllRanges(), i > r ? (e.addRange(t), e.extend(o.node, o.offset)) : (t.setEnd(o.node, o.offset), e.addRange(t)));
      }
    }
    for (t = [], e = n; e = e.parentNode; ) e.nodeType === 1 && t.push({ element: e, left: e.scrollLeft, top: e.scrollTop });
    for (typeof n.focus == "function" && n.focus(), n = 0; n < t.length; n++) e = t[n], e.element.scrollLeft = e.left, e.element.scrollTop = e.top;
  }
}
var Zf = Qe && "documentMode" in document && 11 >= document.documentMode, It = null, mi = null, Nn = null, hi = !1;
function cu(e, t, n) {
  var r = n.window === n ? n.document : n.nodeType === 9 ? n : n.ownerDocument;
  hi || It == null || It !== jr(r) || (r = It, "selectionStart" in r && oo(r) ? r = { start: r.selectionStart, end: r.selectionEnd } : (r = (r.ownerDocument && r.ownerDocument.defaultView || window).getSelection(), r = { anchorNode: r.anchorNode, anchorOffset: r.anchorOffset, focusNode: r.focusNode, focusOffset: r.focusOffset }), Nn && Un(Nn, r) || (Nn = r, r = Ar(mi, "onSelect"), 0 < r.length && (t = new ro("onSelect", "select", null, t, n), e.push({ event: t, listeners: r }), t.target = It)));
}
function fr(e, t) {
  var n = {};
  return n[e.toLowerCase()] = t.toLowerCase(), n["Webkit" + e] = "webkit" + t, n["Moz" + e] = "moz" + t, n;
}
var Ft = { animationend: fr("Animation", "AnimationEnd"), animationiteration: fr("Animation", "AnimationIteration"), animationstart: fr("Animation", "AnimationStart"), transitionend: fr("Transition", "TransitionEnd") }, jl = {}, Js = {};
Qe && (Js = document.createElement("div").style, "AnimationEvent" in window || (delete Ft.animationend.animation, delete Ft.animationiteration.animation, delete Ft.animationstart.animation), "TransitionEvent" in window || delete Ft.transitionend.transition);
function ul(e) {
  if (jl[e]) return jl[e];
  if (!Ft[e]) return e;
  var t = Ft[e], n;
  for (n in t) if (t.hasOwnProperty(n) && n in Js) return jl[e] = t[n];
  return e;
}
var qs = ul("animationend"), bs = ul("animationiteration"), ea = ul("animationstart"), ta = ul("transitionend"), na = /* @__PURE__ */ new Map(), fu = "abort auxClick cancel canPlay canPlayThrough click close contextMenu copy cut drag dragEnd dragEnter dragExit dragLeave dragOver dragStart drop durationChange emptied encrypted ended error gotPointerCapture input invalid keyDown keyPress keyUp load loadedData loadedMetadata loadStart lostPointerCapture mouseDown mouseMove mouseOut mouseOver mouseUp paste pause play playing pointerCancel pointerDown pointerMove pointerOut pointerOver pointerUp progress rateChange reset resize seeked seeking stalled submit suspend timeUpdate touchCancel touchEnd touchStart volumeChange scroll toggle touchMove waiting wheel".split(" ");
function pt(e, t) {
  na.set(e, t), Lt(t, [e]);
}
for (var Ol = 0; Ol < fu.length; Ol++) {
  var Dl = fu[Ol], Jf = Dl.toLowerCase(), qf = Dl[0].toUpperCase() + Dl.slice(1);
  pt(Jf, "on" + qf);
}
pt(qs, "onAnimationEnd");
pt(bs, "onAnimationIteration");
pt(ea, "onAnimationStart");
pt("dblclick", "onDoubleClick");
pt("focusin", "onFocus");
pt("focusout", "onBlur");
pt(ta, "onTransitionEnd");
Jt("onMouseEnter", ["mouseout", "mouseover"]);
Jt("onMouseLeave", ["mouseout", "mouseover"]);
Jt("onPointerEnter", ["pointerout", "pointerover"]);
Jt("onPointerLeave", ["pointerout", "pointerover"]);
Lt("onChange", "change click focusin focusout input keydown keyup selectionchange".split(" "));
Lt("onSelect", "focusout contextmenu dragend focusin keydown keyup mousedown mouseup selectionchange".split(" "));
Lt("onBeforeInput", ["compositionend", "keypress", "textInput", "paste"]);
Lt("onCompositionEnd", "compositionend focusout keydown keypress keyup mousedown".split(" "));
Lt("onCompositionStart", "compositionstart focusout keydown keypress keyup mousedown".split(" "));
Lt("onCompositionUpdate", "compositionupdate focusout keydown keypress keyup mousedown".split(" "));
var Sn = "abort canplay canplaythrough durationchange emptied encrypted ended error loadeddata loadedmetadata loadstart pause play playing progress ratechange resize seeked seeking stalled suspend timeupdate volumechange waiting".split(" "), bf = new Set("cancel close invalid load scroll toggle".split(" ").concat(Sn));
function du(e, t, n) {
  var r = e.type || "unknown-event";
  e.currentTarget = n, Zc(r, t, void 0, e), e.currentTarget = null;
}
function ra(e, t) {
  t = (t & 4) !== 0;
  for (var n = 0; n < e.length; n++) {
    var r = e[n], l = r.event;
    r = r.listeners;
    e: {
      var i = void 0;
      if (t) for (var o = r.length - 1; 0 <= o; o--) {
        var u = r[o], s = u.instance, c = u.currentTarget;
        if (u = u.listener, s !== i && l.isPropagationStopped()) break e;
        du(l, u, c), i = s;
      }
      else for (o = 0; o < r.length; o++) {
        if (u = r[o], s = u.instance, c = u.currentTarget, u = u.listener, s !== i && l.isPropagationStopped()) break e;
        du(l, u, c), i = s;
      }
    }
  }
  if (Dr) throw e = ci, Dr = !1, ci = null, e;
}
function I(e, t) {
  var n = t[ki];
  n === void 0 && (n = t[ki] = /* @__PURE__ */ new Set());
  var r = e + "__bubble";
  n.has(r) || (la(t, e, 2, !1), n.add(r));
}
function Il(e, t, n) {
  var r = 0;
  t && (r |= 4), la(n, e, r, t);
}
var dr = "_reactListening" + Math.random().toString(36).slice(2);
function An(e) {
  if (!e[dr]) {
    e[dr] = !0, fs.forEach(function(n) {
      n !== "selectionchange" && (bf.has(n) || Il(n, !1, e), Il(n, !0, e));
    });
    var t = e.nodeType === 9 ? e : e.ownerDocument;
    t === null || t[dr] || (t[dr] = !0, Il("selectionchange", !1, t));
  }
}
function la(e, t, n, r) {
  switch (Vs(t)) {
    case 1:
      var l = pf;
      break;
    case 4:
      l = mf;
      break;
    default:
      l = to;
  }
  n = l.bind(null, t, n, e), l = void 0, !ai || t !== "touchstart" && t !== "touchmove" && t !== "wheel" || (l = !0), r ? l !== void 0 ? e.addEventListener(t, n, { capture: !0, passive: l }) : e.addEventListener(t, n, !0) : l !== void 0 ? e.addEventListener(t, n, { passive: l }) : e.addEventListener(t, n, !1);
}
function Fl(e, t, n, r, l) {
  var i = r;
  if (!(t & 1) && !(t & 2) && r !== null) e: for (; ; ) {
    if (r === null) return;
    var o = r.tag;
    if (o === 3 || o === 4) {
      var u = r.stateNode.containerInfo;
      if (u === l || u.nodeType === 8 && u.parentNode === l) break;
      if (o === 4) for (o = r.return; o !== null; ) {
        var s = o.tag;
        if ((s === 3 || s === 4) && (s = o.stateNode.containerInfo, s === l || s.nodeType === 8 && s.parentNode === l)) return;
        o = o.return;
      }
      for (; u !== null; ) {
        if (o = kt(u), o === null) return;
        if (s = o.tag, s === 5 || s === 6) {
          r = i = o;
          continue e;
        }
        u = u.parentNode;
      }
    }
    r = r.return;
  }
  zs(function() {
    var c = i, h = Ji(n), m = [];
    e: {
      var p = na.get(e);
      if (p !== void 0) {
        var g = ro, w = e;
        switch (e) {
          case "keypress":
            if (Cr(n) === 0) break e;
          case "keydown":
          case "keyup":
            g = Tf;
            break;
          case "focusin":
            w = "focus", g = Ll;
            break;
          case "focusout":
            w = "blur", g = Ll;
            break;
          case "beforeblur":
          case "afterblur":
            g = Ll;
            break;
          case "click":
            if (n.button === 2) break e;
          case "auxclick":
          case "dblclick":
          case "mousedown":
          case "mousemove":
          case "mouseup":
          case "mouseout":
          case "mouseover":
          case "contextmenu":
            g = eu;
            break;
          case "drag":
          case "dragend":
          case "dragenter":
          case "dragexit":
          case "dragleave":
          case "dragover":
          case "dragstart":
          case "drop":
            g = yf;
            break;
          case "touchcancel":
          case "touchend":
          case "touchmove":
          case "touchstart":
            g = Rf;
            break;
          case qs:
          case bs:
          case ea:
            g = kf;
            break;
          case ta:
            g = Of;
            break;
          case "scroll":
            g = hf;
            break;
          case "wheel":
            g = If;
            break;
          case "copy":
          case "cut":
          case "paste":
            g = xf;
            break;
          case "gotpointercapture":
          case "lostpointercapture":
          case "pointercancel":
          case "pointerdown":
          case "pointermove":
          case "pointerout":
          case "pointerover":
          case "pointerup":
            g = nu;
        }
        var k = (t & 4) !== 0, O = !k && e === "scroll", d = k ? p !== null ? p + "Capture" : null : p;
        k = [];
        for (var a = c, f; a !== null; ) {
          f = a;
          var v = f.stateNode;
          if (f.tag === 5 && v !== null && (f = v, d !== null && (v = On(a, d), v != null && k.push(Vn(a, v, f)))), O) break;
          a = a.return;
        }
        0 < k.length && (p = new g(p, w, null, n, h), m.push({ event: p, listeners: k }));
      }
    }
    if (!(t & 7)) {
      e: {
        if (p = e === "mouseover" || e === "pointerover", g = e === "mouseout" || e === "pointerout", p && n !== ui && (w = n.relatedTarget || n.fromElement) && (kt(w) || w[Ye])) break e;
        if ((g || p) && (p = h.window === h ? h : (p = h.ownerDocument) ? p.defaultView || p.parentWindow : window, g ? (w = n.relatedTarget || n.toElement, g = c, w = w ? kt(w) : null, w !== null && (O = Mt(w), w !== O || w.tag !== 5 && w.tag !== 6) && (w = null)) : (g = null, w = c), g !== w)) {
          if (k = eu, v = "onMouseLeave", d = "onMouseEnter", a = "mouse", (e === "pointerout" || e === "pointerover") && (k = nu, v = "onPointerLeave", d = "onPointerEnter", a = "pointer"), O = g == null ? p : $t(g), f = w == null ? p : $t(w), p = new k(v, a + "leave", g, n, h), p.target = O, p.relatedTarget = f, v = null, kt(h) === c && (k = new k(d, a + "enter", w, n, h), k.target = f, k.relatedTarget = O, v = k), O = v, g && w) t: {
            for (k = g, d = w, a = 0, f = k; f; f = Rt(f)) a++;
            for (f = 0, v = d; v; v = Rt(v)) f++;
            for (; 0 < a - f; ) k = Rt(k), a--;
            for (; 0 < f - a; ) d = Rt(d), f--;
            for (; a--; ) {
              if (k === d || d !== null && k === d.alternate) break t;
              k = Rt(k), d = Rt(d);
            }
            k = null;
          }
          else k = null;
          g !== null && pu(m, p, g, k, !1), w !== null && O !== null && pu(m, O, w, k, !0);
        }
      }
      e: {
        if (p = c ? $t(c) : window, g = p.nodeName && p.nodeName.toLowerCase(), g === "select" || g === "input" && p.type === "file") var x = Hf;
        else if (iu(p)) if (Ks) x = Kf;
        else {
          x = Qf;
          var _ = Wf;
        }
        else (g = p.nodeName) && g.toLowerCase() === "input" && (p.type === "checkbox" || p.type === "radio") && (x = Yf);
        if (x && (x = x(e, c))) {
          Ys(m, x, n, h);
          break e;
        }
        _ && _(e, p, c), e === "focusout" && (_ = p._wrapperState) && _.controlled && p.type === "number" && ni(p, "number", p.value);
      }
      switch (_ = c ? $t(c) : window, e) {
        case "focusin":
          (iu(_) || _.contentEditable === "true") && (It = _, mi = c, Nn = null);
          break;
        case "focusout":
          Nn = mi = It = null;
          break;
        case "mousedown":
          hi = !0;
          break;
        case "contextmenu":
        case "mouseup":
        case "dragend":
          hi = !1, cu(m, n, h);
          break;
        case "selectionchange":
          if (Zf) break;
        case "keydown":
        case "keyup":
          cu(m, n, h);
      }
      var C;
      if (io) e: {
        switch (e) {
          case "compositionstart":
            var N = "onCompositionStart";
            break e;
          case "compositionend":
            N = "onCompositionEnd";
            break e;
          case "compositionupdate":
            N = "onCompositionUpdate";
            break e;
        }
        N = void 0;
      }
      else Dt ? Ws(e, n) && (N = "onCompositionEnd") : e === "keydown" && n.keyCode === 229 && (N = "onCompositionStart");
      N && (Hs && n.locale !== "ko" && (Dt || N !== "onCompositionStart" ? N === "onCompositionEnd" && Dt && (C = Bs()) : (tt = h, no = "value" in tt ? tt.value : tt.textContent, Dt = !0)), _ = Ar(c, N), 0 < _.length && (N = new tu(N, e, null, n, h), m.push({ event: N, listeners: _ }), C ? N.data = C : (C = Qs(n), C !== null && (N.data = C)))), (C = $f ? Uf(e, n) : Af(e, n)) && (c = Ar(c, "onBeforeInput"), 0 < c.length && (h = new tu("onBeforeInput", "beforeinput", null, n, h), m.push({ event: h, listeners: c }), h.data = C));
    }
    ra(m, t);
  });
}
function Vn(e, t, n) {
  return { instance: e, listener: t, currentTarget: n };
}
function Ar(e, t) {
  for (var n = t + "Capture", r = []; e !== null; ) {
    var l = e, i = l.stateNode;
    l.tag === 5 && i !== null && (l = i, i = On(e, n), i != null && r.unshift(Vn(e, i, l)), i = On(e, t), i != null && r.push(Vn(e, i, l))), e = e.return;
  }
  return r;
}
function Rt(e) {
  if (e === null) return null;
  do
    e = e.return;
  while (e && e.tag !== 5);
  return e || null;
}
function pu(e, t, n, r, l) {
  for (var i = t._reactName, o = []; n !== null && n !== r; ) {
    var u = n, s = u.alternate, c = u.stateNode;
    if (s !== null && s === r) break;
    u.tag === 5 && c !== null && (u = c, l ? (s = On(n, i), s != null && o.unshift(Vn(n, s, u))) : l || (s = On(n, i), s != null && o.push(Vn(n, s, u)))), n = n.return;
  }
  o.length !== 0 && e.push({ event: t, listeners: o });
}
var ed = /\r\n?/g, td = /\u0000|\uFFFD/g;
function mu(e) {
  return (typeof e == "string" ? e : "" + e).replace(ed, `
`).replace(td, "");
}
function pr(e, t, n) {
  if (t = mu(t), mu(e) !== t && n) throw Error(y(425));
}
function Vr() {
}
var vi = null, yi = null;
function gi(e, t) {
  return e === "textarea" || e === "noscript" || typeof t.children == "string" || typeof t.children == "number" || typeof t.dangerouslySetInnerHTML == "object" && t.dangerouslySetInnerHTML !== null && t.dangerouslySetInnerHTML.__html != null;
}
var wi = typeof setTimeout == "function" ? setTimeout : void 0, nd = typeof clearTimeout == "function" ? clearTimeout : void 0, hu = typeof Promise == "function" ? Promise : void 0, rd = typeof queueMicrotask == "function" ? queueMicrotask : typeof hu < "u" ? function(e) {
  return hu.resolve(null).then(e).catch(ld);
} : wi;
function ld(e) {
  setTimeout(function() {
    throw e;
  });
}
function $l(e, t) {
  var n = t, r = 0;
  do {
    var l = n.nextSibling;
    if (e.removeChild(n), l && l.nodeType === 8) if (n = l.data, n === "/$") {
      if (r === 0) {
        e.removeChild(l), Fn(t);
        return;
      }
      r--;
    } else n !== "$" && n !== "$?" && n !== "$!" || r++;
    n = l;
  } while (n);
  Fn(t);
}
function ot(e) {
  for (; e != null; e = e.nextSibling) {
    var t = e.nodeType;
    if (t === 1 || t === 3) break;
    if (t === 8) {
      if (t = e.data, t === "$" || t === "$!" || t === "$?") break;
      if (t === "/$") return null;
    }
  }
  return e;
}
function vu(e) {
  e = e.previousSibling;
  for (var t = 0; e; ) {
    if (e.nodeType === 8) {
      var n = e.data;
      if (n === "$" || n === "$!" || n === "$?") {
        if (t === 0) return e;
        t--;
      } else n === "/$" && t++;
    }
    e = e.previousSibling;
  }
  return null;
}
var un = Math.random().toString(36).slice(2), Fe = "__reactFiber$" + un, Bn = "__reactProps$" + un, Ye = "__reactContainer$" + un, ki = "__reactEvents$" + un, id = "__reactListeners$" + un, od = "__reactHandles$" + un;
function kt(e) {
  var t = e[Fe];
  if (t) return t;
  for (var n = e.parentNode; n; ) {
    if (t = n[Ye] || n[Fe]) {
      if (n = t.alternate, t.child !== null || n !== null && n.child !== null) for (e = vu(e); e !== null; ) {
        if (n = e[Fe]) return n;
        e = vu(e);
      }
      return t;
    }
    e = n, n = e.parentNode;
  }
  return null;
}
function qn(e) {
  return e = e[Fe] || e[Ye], !e || e.tag !== 5 && e.tag !== 6 && e.tag !== 13 && e.tag !== 3 ? null : e;
}
function $t(e) {
  if (e.tag === 5 || e.tag === 6) return e.stateNode;
  throw Error(y(33));
}
function sl(e) {
  return e[Bn] || null;
}
var Si = [], Ut = -1;
function mt(e) {
  return { current: e };
}
function F(e) {
  0 > Ut || (e.current = Si[Ut], Si[Ut] = null, Ut--);
}
function D(e, t) {
  Ut++, Si[Ut] = e.current, e.current = t;
}
var dt = {}, ie = mt(dt), de = mt(!1), Ct = dt;
function qt(e, t) {
  var n = e.type.contextTypes;
  if (!n) return dt;
  var r = e.stateNode;
  if (r && r.__reactInternalMemoizedUnmaskedChildContext === t) return r.__reactInternalMemoizedMaskedChildContext;
  var l = {}, i;
  for (i in n) l[i] = t[i];
  return r && (e = e.stateNode, e.__reactInternalMemoizedUnmaskedChildContext = t, e.__reactInternalMemoizedMaskedChildContext = l), l;
}
function pe(e) {
  return e = e.childContextTypes, e != null;
}
function Br() {
  F(de), F(ie);
}
function yu(e, t, n) {
  if (ie.current !== dt) throw Error(y(168));
  D(ie, t), D(de, n);
}
function ia(e, t, n) {
  var r = e.stateNode;
  if (t = t.childContextTypes, typeof r.getChildContext != "function") return n;
  r = r.getChildContext();
  for (var l in r) if (!(l in t)) throw Error(y(108, Hc(e) || "Unknown", l));
  return V({}, n, r);
}
function Hr(e) {
  return e = (e = e.stateNode) && e.__reactInternalMemoizedMergedChildContext || dt, Ct = ie.current, D(ie, e), D(de, de.current), !0;
}
function gu(e, t, n) {
  var r = e.stateNode;
  if (!r) throw Error(y(169));
  n ? (e = ia(e, t, Ct), r.__reactInternalMemoizedMergedChildContext = e, F(de), F(ie), D(ie, e)) : F(de), D(de, n);
}
var Ve = null, al = !1, Ul = !1;
function oa(e) {
  Ve === null ? Ve = [e] : Ve.push(e);
}
function ud(e) {
  al = !0, oa(e);
}
function ht() {
  if (!Ul && Ve !== null) {
    Ul = !0;
    var e = 0, t = j;
    try {
      var n = Ve;
      for (j = 1; e < n.length; e++) {
        var r = n[e];
        do
          r = r(!0);
        while (r !== null);
      }
      Ve = null, al = !1;
    } catch (l) {
      throw Ve !== null && (Ve = Ve.slice(e + 1)), Ms(qi, ht), l;
    } finally {
      j = t, Ul = !1;
    }
  }
  return null;
}
var At = [], Vt = 0, Wr = null, Qr = 0, Se = [], xe = 0, Nt = null, Be = 1, He = "";
function gt(e, t) {
  At[Vt++] = Qr, At[Vt++] = Wr, Wr = e, Qr = t;
}
function ua(e, t, n) {
  Se[xe++] = Be, Se[xe++] = He, Se[xe++] = Nt, Nt = e;
  var r = Be;
  e = He;
  var l = 32 - Re(r) - 1;
  r &= ~(1 << l), n += 1;
  var i = 32 - Re(t) + l;
  if (30 < i) {
    var o = l - l % 5;
    i = (r & (1 << o) - 1).toString(32), r >>= o, l -= o, Be = 1 << 32 - Re(t) + l | n << l | r, He = i + e;
  } else Be = 1 << i | n << l | r, He = e;
}
function uo(e) {
  e.return !== null && (gt(e, 1), ua(e, 1, 0));
}
function so(e) {
  for (; e === Wr; ) Wr = At[--Vt], At[Vt] = null, Qr = At[--Vt], At[Vt] = null;
  for (; e === Nt; ) Nt = Se[--xe], Se[xe] = null, He = Se[--xe], Se[xe] = null, Be = Se[--xe], Se[xe] = null;
}
var ye = null, ve = null, $ = !1, Me = null;
function sa(e, t) {
  var n = Ee(5, null, null, 0);
  n.elementType = "DELETED", n.stateNode = t, n.return = e, t = e.deletions, t === null ? (e.deletions = [n], e.flags |= 16) : t.push(n);
}
function wu(e, t) {
  switch (e.tag) {
    case 5:
      var n = e.type;
      return t = t.nodeType !== 1 || n.toLowerCase() !== t.nodeName.toLowerCase() ? null : t, t !== null ? (e.stateNode = t, ye = e, ve = ot(t.firstChild), !0) : !1;
    case 6:
      return t = e.pendingProps === "" || t.nodeType !== 3 ? null : t, t !== null ? (e.stateNode = t, ye = e, ve = null, !0) : !1;
    case 13:
      return t = t.nodeType !== 8 ? null : t, t !== null ? (n = Nt !== null ? { id: Be, overflow: He } : null, e.memoizedState = { dehydrated: t, treeContext: n, retryLane: 1073741824 }, n = Ee(18, null, null, 0), n.stateNode = t, n.return = e, e.child = n, ye = e, ve = null, !0) : !1;
    default:
      return !1;
  }
}
function xi(e) {
  return (e.mode & 1) !== 0 && (e.flags & 128) === 0;
}
function Ei(e) {
  if ($) {
    var t = ve;
    if (t) {
      var n = t;
      if (!wu(e, t)) {
        if (xi(e)) throw Error(y(418));
        t = ot(n.nextSibling);
        var r = ye;
        t && wu(e, t) ? sa(r, n) : (e.flags = e.flags & -4097 | 2, $ = !1, ye = e);
      }
    } else {
      if (xi(e)) throw Error(y(418));
      e.flags = e.flags & -4097 | 2, $ = !1, ye = e;
    }
  }
}
function ku(e) {
  for (e = e.return; e !== null && e.tag !== 5 && e.tag !== 3 && e.tag !== 13; ) e = e.return;
  ye = e;
}
function mr(e) {
  if (e !== ye) return !1;
  if (!$) return ku(e), $ = !0, !1;
  var t;
  if ((t = e.tag !== 3) && !(t = e.tag !== 5) && (t = e.type, t = t !== "head" && t !== "body" && !gi(e.type, e.memoizedProps)), t && (t = ve)) {
    if (xi(e)) throw aa(), Error(y(418));
    for (; t; ) sa(e, t), t = ot(t.nextSibling);
  }
  if (ku(e), e.tag === 13) {
    if (e = e.memoizedState, e = e !== null ? e.dehydrated : null, !e) throw Error(y(317));
    e: {
      for (e = e.nextSibling, t = 0; e; ) {
        if (e.nodeType === 8) {
          var n = e.data;
          if (n === "/$") {
            if (t === 0) {
              ve = ot(e.nextSibling);
              break e;
            }
            t--;
          } else n !== "$" && n !== "$!" && n !== "$?" || t++;
        }
        e = e.nextSibling;
      }
      ve = null;
    }
  } else ve = ye ? ot(e.stateNode.nextSibling) : null;
  return !0;
}
function aa() {
  for (var e = ve; e; ) e = ot(e.nextSibling);
}
function bt() {
  ve = ye = null, $ = !1;
}
function ao(e) {
  Me === null ? Me = [e] : Me.push(e);
}
var sd = Ge.ReactCurrentBatchConfig;
function hn(e, t, n) {
  if (e = n.ref, e !== null && typeof e != "function" && typeof e != "object") {
    if (n._owner) {
      if (n = n._owner, n) {
        if (n.tag !== 1) throw Error(y(309));
        var r = n.stateNode;
      }
      if (!r) throw Error(y(147, e));
      var l = r, i = "" + e;
      return t !== null && t.ref !== null && typeof t.ref == "function" && t.ref._stringRef === i ? t.ref : (t = function(o) {
        var u = l.refs;
        o === null ? delete u[i] : u[i] = o;
      }, t._stringRef = i, t);
    }
    if (typeof e != "string") throw Error(y(284));
    if (!n._owner) throw Error(y(290, e));
  }
  return e;
}
function hr(e, t) {
  throw e = Object.prototype.toString.call(t), Error(y(31, e === "[object Object]" ? "object with keys {" + Object.keys(t).join(", ") + "}" : e));
}
function Su(e) {
  var t = e._init;
  return t(e._payload);
}
function ca(e) {
  function t(d, a) {
    if (e) {
      var f = d.deletions;
      f === null ? (d.deletions = [a], d.flags |= 16) : f.push(a);
    }
  }
  function n(d, a) {
    if (!e) return null;
    for (; a !== null; ) t(d, a), a = a.sibling;
    return null;
  }
  function r(d, a) {
    for (d = /* @__PURE__ */ new Map(); a !== null; ) a.key !== null ? d.set(a.key, a) : d.set(a.index, a), a = a.sibling;
    return d;
  }
  function l(d, a) {
    return d = ct(d, a), d.index = 0, d.sibling = null, d;
  }
  function i(d, a, f) {
    return d.index = f, e ? (f = d.alternate, f !== null ? (f = f.index, f < a ? (d.flags |= 2, a) : f) : (d.flags |= 2, a)) : (d.flags |= 1048576, a);
  }
  function o(d) {
    return e && d.alternate === null && (d.flags |= 2), d;
  }
  function u(d, a, f, v) {
    return a === null || a.tag !== 6 ? (a = Yl(f, d.mode, v), a.return = d, a) : (a = l(a, f), a.return = d, a);
  }
  function s(d, a, f, v) {
    var x = f.type;
    return x === Ot ? h(d, a, f.props.children, v, f.key) : a !== null && (a.elementType === x || typeof x == "object" && x !== null && x.$$typeof === Je && Su(x) === a.type) ? (v = l(a, f.props), v.ref = hn(d, a, f), v.return = d, v) : (v = Rr(f.type, f.key, f.props, null, d.mode, v), v.ref = hn(d, a, f), v.return = d, v);
  }
  function c(d, a, f, v) {
    return a === null || a.tag !== 4 || a.stateNode.containerInfo !== f.containerInfo || a.stateNode.implementation !== f.implementation ? (a = Kl(f, d.mode, v), a.return = d, a) : (a = l(a, f.children || []), a.return = d, a);
  }
  function h(d, a, f, v, x) {
    return a === null || a.tag !== 7 ? (a = _t(f, d.mode, v, x), a.return = d, a) : (a = l(a, f), a.return = d, a);
  }
  function m(d, a, f) {
    if (typeof a == "string" && a !== "" || typeof a == "number") return a = Yl("" + a, d.mode, f), a.return = d, a;
    if (typeof a == "object" && a !== null) {
      switch (a.$$typeof) {
        case lr:
          return f = Rr(a.type, a.key, a.props, null, d.mode, f), f.ref = hn(d, null, a), f.return = d, f;
        case jt:
          return a = Kl(a, d.mode, f), a.return = d, a;
        case Je:
          var v = a._init;
          return m(d, v(a._payload), f);
      }
      if (wn(a) || cn(a)) return a = _t(a, d.mode, f, null), a.return = d, a;
      hr(d, a);
    }
    return null;
  }
  function p(d, a, f, v) {
    var x = a !== null ? a.key : null;
    if (typeof f == "string" && f !== "" || typeof f == "number") return x !== null ? null : u(d, a, "" + f, v);
    if (typeof f == "object" && f !== null) {
      switch (f.$$typeof) {
        case lr:
          return f.key === x ? s(d, a, f, v) : null;
        case jt:
          return f.key === x ? c(d, a, f, v) : null;
        case Je:
          return x = f._init, p(
            d,
            a,
            x(f._payload),
            v
          );
      }
      if (wn(f) || cn(f)) return x !== null ? null : h(d, a, f, v, null);
      hr(d, f);
    }
    return null;
  }
  function g(d, a, f, v, x) {
    if (typeof v == "string" && v !== "" || typeof v == "number") return d = d.get(f) || null, u(a, d, "" + v, x);
    if (typeof v == "object" && v !== null) {
      switch (v.$$typeof) {
        case lr:
          return d = d.get(v.key === null ? f : v.key) || null, s(a, d, v, x);
        case jt:
          return d = d.get(v.key === null ? f : v.key) || null, c(a, d, v, x);
        case Je:
          var _ = v._init;
          return g(d, a, f, _(v._payload), x);
      }
      if (wn(v) || cn(v)) return d = d.get(f) || null, h(a, d, v, x, null);
      hr(a, v);
    }
    return null;
  }
  function w(d, a, f, v) {
    for (var x = null, _ = null, C = a, N = a = 0, H = null; C !== null && N < f.length; N++) {
      C.index > N ? (H = C, C = null) : H = C.sibling;
      var M = p(d, C, f[N], v);
      if (M === null) {
        C === null && (C = H);
        break;
      }
      e && C && M.alternate === null && t(d, C), a = i(M, a, N), _ === null ? x = M : _.sibling = M, _ = M, C = H;
    }
    if (N === f.length) return n(d, C), $ && gt(d, N), x;
    if (C === null) {
      for (; N < f.length; N++) C = m(d, f[N], v), C !== null && (a = i(C, a, N), _ === null ? x = C : _.sibling = C, _ = C);
      return $ && gt(d, N), x;
    }
    for (C = r(d, C); N < f.length; N++) H = g(C, d, N, f[N], v), H !== null && (e && H.alternate !== null && C.delete(H.key === null ? N : H.key), a = i(H, a, N), _ === null ? x = H : _.sibling = H, _ = H);
    return e && C.forEach(function(ze) {
      return t(d, ze);
    }), $ && gt(d, N), x;
  }
  function k(d, a, f, v) {
    var x = cn(f);
    if (typeof x != "function") throw Error(y(150));
    if (f = x.call(f), f == null) throw Error(y(151));
    for (var _ = x = null, C = a, N = a = 0, H = null, M = f.next(); C !== null && !M.done; N++, M = f.next()) {
      C.index > N ? (H = C, C = null) : H = C.sibling;
      var ze = p(d, C, M.value, v);
      if (ze === null) {
        C === null && (C = H);
        break;
      }
      e && C && ze.alternate === null && t(d, C), a = i(ze, a, N), _ === null ? x = ze : _.sibling = ze, _ = ze, C = H;
    }
    if (M.done) return n(
      d,
      C
    ), $ && gt(d, N), x;
    if (C === null) {
      for (; !M.done; N++, M = f.next()) M = m(d, M.value, v), M !== null && (a = i(M, a, N), _ === null ? x = M : _.sibling = M, _ = M);
      return $ && gt(d, N), x;
    }
    for (C = r(d, C); !M.done; N++, M = f.next()) M = g(C, d, N, M.value, v), M !== null && (e && M.alternate !== null && C.delete(M.key === null ? N : M.key), a = i(M, a, N), _ === null ? x = M : _.sibling = M, _ = M);
    return e && C.forEach(function(sn) {
      return t(d, sn);
    }), $ && gt(d, N), x;
  }
  function O(d, a, f, v) {
    if (typeof f == "object" && f !== null && f.type === Ot && f.key === null && (f = f.props.children), typeof f == "object" && f !== null) {
      switch (f.$$typeof) {
        case lr:
          e: {
            for (var x = f.key, _ = a; _ !== null; ) {
              if (_.key === x) {
                if (x = f.type, x === Ot) {
                  if (_.tag === 7) {
                    n(d, _.sibling), a = l(_, f.props.children), a.return = d, d = a;
                    break e;
                  }
                } else if (_.elementType === x || typeof x == "object" && x !== null && x.$$typeof === Je && Su(x) === _.type) {
                  n(d, _.sibling), a = l(_, f.props), a.ref = hn(d, _, f), a.return = d, d = a;
                  break e;
                }
                n(d, _);
                break;
              } else t(d, _);
              _ = _.sibling;
            }
            f.type === Ot ? (a = _t(f.props.children, d.mode, v, f.key), a.return = d, d = a) : (v = Rr(f.type, f.key, f.props, null, d.mode, v), v.ref = hn(d, a, f), v.return = d, d = v);
          }
          return o(d);
        case jt:
          e: {
            for (_ = f.key; a !== null; ) {
              if (a.key === _) if (a.tag === 4 && a.stateNode.containerInfo === f.containerInfo && a.stateNode.implementation === f.implementation) {
                n(d, a.sibling), a = l(a, f.children || []), a.return = d, d = a;
                break e;
              } else {
                n(d, a);
                break;
              }
              else t(d, a);
              a = a.sibling;
            }
            a = Kl(f, d.mode, v), a.return = d, d = a;
          }
          return o(d);
        case Je:
          return _ = f._init, O(d, a, _(f._payload), v);
      }
      if (wn(f)) return w(d, a, f, v);
      if (cn(f)) return k(d, a, f, v);
      hr(d, f);
    }
    return typeof f == "string" && f !== "" || typeof f == "number" ? (f = "" + f, a !== null && a.tag === 6 ? (n(d, a.sibling), a = l(a, f), a.return = d, d = a) : (n(d, a), a = Yl(f, d.mode, v), a.return = d, d = a), o(d)) : n(d, a);
  }
  return O;
}
var en = ca(!0), fa = ca(!1), Yr = mt(null), Kr = null, Bt = null, co = null;
function fo() {
  co = Bt = Kr = null;
}
function po(e) {
  var t = Yr.current;
  F(Yr), e._currentValue = t;
}
function _i(e, t, n) {
  for (; e !== null; ) {
    var r = e.alternate;
    if ((e.childLanes & t) !== t ? (e.childLanes |= t, r !== null && (r.childLanes |= t)) : r !== null && (r.childLanes & t) !== t && (r.childLanes |= t), e === n) break;
    e = e.return;
  }
}
function Gt(e, t) {
  Kr = e, co = Bt = null, e = e.dependencies, e !== null && e.firstContext !== null && (e.lanes & t && (fe = !0), e.firstContext = null);
}
function Ce(e) {
  var t = e._currentValue;
  if (co !== e) if (e = { context: e, memoizedValue: t, next: null }, Bt === null) {
    if (Kr === null) throw Error(y(308));
    Bt = e, Kr.dependencies = { lanes: 0, firstContext: e };
  } else Bt = Bt.next = e;
  return t;
}
var St = null;
function mo(e) {
  St === null ? St = [e] : St.push(e);
}
function da(e, t, n, r) {
  var l = t.interleaved;
  return l === null ? (n.next = n, mo(t)) : (n.next = l.next, l.next = n), t.interleaved = n, Ke(e, r);
}
function Ke(e, t) {
  e.lanes |= t;
  var n = e.alternate;
  for (n !== null && (n.lanes |= t), n = e, e = e.return; e !== null; ) e.childLanes |= t, n = e.alternate, n !== null && (n.childLanes |= t), n = e, e = e.return;
  return n.tag === 3 ? n.stateNode : null;
}
var qe = !1;
function ho(e) {
  e.updateQueue = { baseState: e.memoizedState, firstBaseUpdate: null, lastBaseUpdate: null, shared: { pending: null, interleaved: null, lanes: 0 }, effects: null };
}
function pa(e, t) {
  e = e.updateQueue, t.updateQueue === e && (t.updateQueue = { baseState: e.baseState, firstBaseUpdate: e.firstBaseUpdate, lastBaseUpdate: e.lastBaseUpdate, shared: e.shared, effects: e.effects });
}
function We(e, t) {
  return { eventTime: e, lane: t, tag: 0, payload: null, callback: null, next: null };
}
function ut(e, t, n) {
  var r = e.updateQueue;
  if (r === null) return null;
  if (r = r.shared, R & 2) {
    var l = r.pending;
    return l === null ? t.next = t : (t.next = l.next, l.next = t), r.pending = t, Ke(e, n);
  }
  return l = r.interleaved, l === null ? (t.next = t, mo(r)) : (t.next = l.next, l.next = t), r.interleaved = t, Ke(e, n);
}
function Nr(e, t, n) {
  if (t = t.updateQueue, t !== null && (t = t.shared, (n & 4194240) !== 0)) {
    var r = t.lanes;
    r &= e.pendingLanes, n |= r, t.lanes = n, bi(e, n);
  }
}
function xu(e, t) {
  var n = e.updateQueue, r = e.alternate;
  if (r !== null && (r = r.updateQueue, n === r)) {
    var l = null, i = null;
    if (n = n.firstBaseUpdate, n !== null) {
      do {
        var o = { eventTime: n.eventTime, lane: n.lane, tag: n.tag, payload: n.payload, callback: n.callback, next: null };
        i === null ? l = i = o : i = i.next = o, n = n.next;
      } while (n !== null);
      i === null ? l = i = t : i = i.next = t;
    } else l = i = t;
    n = { baseState: r.baseState, firstBaseUpdate: l, lastBaseUpdate: i, shared: r.shared, effects: r.effects }, e.updateQueue = n;
    return;
  }
  e = n.lastBaseUpdate, e === null ? n.firstBaseUpdate = t : e.next = t, n.lastBaseUpdate = t;
}
function Xr(e, t, n, r) {
  var l = e.updateQueue;
  qe = !1;
  var i = l.firstBaseUpdate, o = l.lastBaseUpdate, u = l.shared.pending;
  if (u !== null) {
    l.shared.pending = null;
    var s = u, c = s.next;
    s.next = null, o === null ? i = c : o.next = c, o = s;
    var h = e.alternate;
    h !== null && (h = h.updateQueue, u = h.lastBaseUpdate, u !== o && (u === null ? h.firstBaseUpdate = c : u.next = c, h.lastBaseUpdate = s));
  }
  if (i !== null) {
    var m = l.baseState;
    o = 0, h = c = s = null, u = i;
    do {
      var p = u.lane, g = u.eventTime;
      if ((r & p) === p) {
        h !== null && (h = h.next = {
          eventTime: g,
          lane: 0,
          tag: u.tag,
          payload: u.payload,
          callback: u.callback,
          next: null
        });
        e: {
          var w = e, k = u;
          switch (p = t, g = n, k.tag) {
            case 1:
              if (w = k.payload, typeof w == "function") {
                m = w.call(g, m, p);
                break e;
              }
              m = w;
              break e;
            case 3:
              w.flags = w.flags & -65537 | 128;
            case 0:
              if (w = k.payload, p = typeof w == "function" ? w.call(g, m, p) : w, p == null) break e;
              m = V({}, m, p);
              break e;
            case 2:
              qe = !0;
          }
        }
        u.callback !== null && u.lane !== 0 && (e.flags |= 64, p = l.effects, p === null ? l.effects = [u] : p.push(u));
      } else g = { eventTime: g, lane: p, tag: u.tag, payload: u.payload, callback: u.callback, next: null }, h === null ? (c = h = g, s = m) : h = h.next = g, o |= p;
      if (u = u.next, u === null) {
        if (u = l.shared.pending, u === null) break;
        p = u, u = p.next, p.next = null, l.lastBaseUpdate = p, l.shared.pending = null;
      }
    } while (!0);
    if (h === null && (s = m), l.baseState = s, l.firstBaseUpdate = c, l.lastBaseUpdate = h, t = l.shared.interleaved, t !== null) {
      l = t;
      do
        o |= l.lane, l = l.next;
      while (l !== t);
    } else i === null && (l.shared.lanes = 0);
    Pt |= o, e.lanes = o, e.memoizedState = m;
  }
}
function Eu(e, t, n) {
  if (e = t.effects, t.effects = null, e !== null) for (t = 0; t < e.length; t++) {
    var r = e[t], l = r.callback;
    if (l !== null) {
      if (r.callback = null, r = n, typeof l != "function") throw Error(y(191, l));
      l.call(r);
    }
  }
}
var bn = {}, Ue = mt(bn), Hn = mt(bn), Wn = mt(bn);
function xt(e) {
  if (e === bn) throw Error(y(174));
  return e;
}
function vo(e, t) {
  switch (D(Wn, t), D(Hn, e), D(Ue, bn), e = t.nodeType, e) {
    case 9:
    case 11:
      t = (t = t.documentElement) ? t.namespaceURI : li(null, "");
      break;
    default:
      e = e === 8 ? t.parentNode : t, t = e.namespaceURI || null, e = e.tagName, t = li(t, e);
  }
  F(Ue), D(Ue, t);
}
function tn() {
  F(Ue), F(Hn), F(Wn);
}
function ma(e) {
  xt(Wn.current);
  var t = xt(Ue.current), n = li(t, e.type);
  t !== n && (D(Hn, e), D(Ue, n));
}
function yo(e) {
  Hn.current === e && (F(Ue), F(Hn));
}
var U = mt(0);
function Gr(e) {
  for (var t = e; t !== null; ) {
    if (t.tag === 13) {
      var n = t.memoizedState;
      if (n !== null && (n = n.dehydrated, n === null || n.data === "$?" || n.data === "$!")) return t;
    } else if (t.tag === 19 && t.memoizedProps.revealOrder !== void 0) {
      if (t.flags & 128) return t;
    } else if (t.child !== null) {
      t.child.return = t, t = t.child;
      continue;
    }
    if (t === e) break;
    for (; t.sibling === null; ) {
      if (t.return === null || t.return === e) return null;
      t = t.return;
    }
    t.sibling.return = t.return, t = t.sibling;
  }
  return null;
}
var Al = [];
function go() {
  for (var e = 0; e < Al.length; e++) Al[e]._workInProgressVersionPrimary = null;
  Al.length = 0;
}
var zr = Ge.ReactCurrentDispatcher, Vl = Ge.ReactCurrentBatchConfig, zt = 0, A = null, K = null, Z = null, Zr = !1, zn = !1, Qn = 0, ad = 0;
function ne() {
  throw Error(y(321));
}
function wo(e, t) {
  if (t === null) return !1;
  for (var n = 0; n < t.length && n < e.length; n++) if (!Oe(e[n], t[n])) return !1;
  return !0;
}
function ko(e, t, n, r, l, i) {
  if (zt = i, A = t, t.memoizedState = null, t.updateQueue = null, t.lanes = 0, zr.current = e === null || e.memoizedState === null ? pd : md, e = n(r, l), zn) {
    i = 0;
    do {
      if (zn = !1, Qn = 0, 25 <= i) throw Error(y(301));
      i += 1, Z = K = null, t.updateQueue = null, zr.current = hd, e = n(r, l);
    } while (zn);
  }
  if (zr.current = Jr, t = K !== null && K.next !== null, zt = 0, Z = K = A = null, Zr = !1, t) throw Error(y(300));
  return e;
}
function So() {
  var e = Qn !== 0;
  return Qn = 0, e;
}
function Ie() {
  var e = { memoizedState: null, baseState: null, baseQueue: null, queue: null, next: null };
  return Z === null ? A.memoizedState = Z = e : Z = Z.next = e, Z;
}
function Ne() {
  if (K === null) {
    var e = A.alternate;
    e = e !== null ? e.memoizedState : null;
  } else e = K.next;
  var t = Z === null ? A.memoizedState : Z.next;
  if (t !== null) Z = t, K = e;
  else {
    if (e === null) throw Error(y(310));
    K = e, e = { memoizedState: K.memoizedState, baseState: K.baseState, baseQueue: K.baseQueue, queue: K.queue, next: null }, Z === null ? A.memoizedState = Z = e : Z = Z.next = e;
  }
  return Z;
}
function Yn(e, t) {
  return typeof t == "function" ? t(e) : t;
}
function Bl(e) {
  var t = Ne(), n = t.queue;
  if (n === null) throw Error(y(311));
  n.lastRenderedReducer = e;
  var r = K, l = r.baseQueue, i = n.pending;
  if (i !== null) {
    if (l !== null) {
      var o = l.next;
      l.next = i.next, i.next = o;
    }
    r.baseQueue = l = i, n.pending = null;
  }
  if (l !== null) {
    i = l.next, r = r.baseState;
    var u = o = null, s = null, c = i;
    do {
      var h = c.lane;
      if ((zt & h) === h) s !== null && (s = s.next = { lane: 0, action: c.action, hasEagerState: c.hasEagerState, eagerState: c.eagerState, next: null }), r = c.hasEagerState ? c.eagerState : e(r, c.action);
      else {
        var m = {
          lane: h,
          action: c.action,
          hasEagerState: c.hasEagerState,
          eagerState: c.eagerState,
          next: null
        };
        s === null ? (u = s = m, o = r) : s = s.next = m, A.lanes |= h, Pt |= h;
      }
      c = c.next;
    } while (c !== null && c !== i);
    s === null ? o = r : s.next = u, Oe(r, t.memoizedState) || (fe = !0), t.memoizedState = r, t.baseState = o, t.baseQueue = s, n.lastRenderedState = r;
  }
  if (e = n.interleaved, e !== null) {
    l = e;
    do
      i = l.lane, A.lanes |= i, Pt |= i, l = l.next;
    while (l !== e);
  } else l === null && (n.lanes = 0);
  return [t.memoizedState, n.dispatch];
}
function Hl(e) {
  var t = Ne(), n = t.queue;
  if (n === null) throw Error(y(311));
  n.lastRenderedReducer = e;
  var r = n.dispatch, l = n.pending, i = t.memoizedState;
  if (l !== null) {
    n.pending = null;
    var o = l = l.next;
    do
      i = e(i, o.action), o = o.next;
    while (o !== l);
    Oe(i, t.memoizedState) || (fe = !0), t.memoizedState = i, t.baseQueue === null && (t.baseState = i), n.lastRenderedState = i;
  }
  return [i, r];
}
function ha() {
}
function va(e, t) {
  var n = A, r = Ne(), l = t(), i = !Oe(r.memoizedState, l);
  if (i && (r.memoizedState = l, fe = !0), r = r.queue, xo(wa.bind(null, n, r, e), [e]), r.getSnapshot !== t || i || Z !== null && Z.memoizedState.tag & 1) {
    if (n.flags |= 2048, Kn(9, ga.bind(null, n, r, l, t), void 0, null), q === null) throw Error(y(349));
    zt & 30 || ya(n, t, l);
  }
  return l;
}
function ya(e, t, n) {
  e.flags |= 16384, e = { getSnapshot: t, value: n }, t = A.updateQueue, t === null ? (t = { lastEffect: null, stores: null }, A.updateQueue = t, t.stores = [e]) : (n = t.stores, n === null ? t.stores = [e] : n.push(e));
}
function ga(e, t, n, r) {
  t.value = n, t.getSnapshot = r, ka(t) && Sa(e);
}
function wa(e, t, n) {
  return n(function() {
    ka(t) && Sa(e);
  });
}
function ka(e) {
  var t = e.getSnapshot;
  e = e.value;
  try {
    var n = t();
    return !Oe(e, n);
  } catch {
    return !0;
  }
}
function Sa(e) {
  var t = Ke(e, 1);
  t !== null && je(t, e, 1, -1);
}
function _u(e) {
  var t = Ie();
  return typeof e == "function" && (e = e()), t.memoizedState = t.baseState = e, e = { pending: null, interleaved: null, lanes: 0, dispatch: null, lastRenderedReducer: Yn, lastRenderedState: e }, t.queue = e, e = e.dispatch = dd.bind(null, A, e), [t.memoizedState, e];
}
function Kn(e, t, n, r) {
  return e = { tag: e, create: t, destroy: n, deps: r, next: null }, t = A.updateQueue, t === null ? (t = { lastEffect: null, stores: null }, A.updateQueue = t, t.lastEffect = e.next = e) : (n = t.lastEffect, n === null ? t.lastEffect = e.next = e : (r = n.next, n.next = e, e.next = r, t.lastEffect = e)), e;
}
function xa() {
  return Ne().memoizedState;
}
function Pr(e, t, n, r) {
  var l = Ie();
  A.flags |= e, l.memoizedState = Kn(1 | t, n, void 0, r === void 0 ? null : r);
}
function cl(e, t, n, r) {
  var l = Ne();
  r = r === void 0 ? null : r;
  var i = void 0;
  if (K !== null) {
    var o = K.memoizedState;
    if (i = o.destroy, r !== null && wo(r, o.deps)) {
      l.memoizedState = Kn(t, n, i, r);
      return;
    }
  }
  A.flags |= e, l.memoizedState = Kn(1 | t, n, i, r);
}
function Cu(e, t) {
  return Pr(8390656, 8, e, t);
}
function xo(e, t) {
  return cl(2048, 8, e, t);
}
function Ea(e, t) {
  return cl(4, 2, e, t);
}
function _a(e, t) {
  return cl(4, 4, e, t);
}
function Ca(e, t) {
  if (typeof t == "function") return e = e(), t(e), function() {
    t(null);
  };
  if (t != null) return e = e(), t.current = e, function() {
    t.current = null;
  };
}
function Na(e, t, n) {
  return n = n != null ? n.concat([e]) : null, cl(4, 4, Ca.bind(null, t, e), n);
}
function Eo() {
}
function za(e, t) {
  var n = Ne();
  t = t === void 0 ? null : t;
  var r = n.memoizedState;
  return r !== null && t !== null && wo(t, r[1]) ? r[0] : (n.memoizedState = [e, t], e);
}
function Pa(e, t) {
  var n = Ne();
  t = t === void 0 ? null : t;
  var r = n.memoizedState;
  return r !== null && t !== null && wo(t, r[1]) ? r[0] : (e = e(), n.memoizedState = [e, t], e);
}
function Ta(e, t, n) {
  return zt & 21 ? (Oe(n, t) || (n = Os(), A.lanes |= n, Pt |= n, e.baseState = !0), t) : (e.baseState && (e.baseState = !1, fe = !0), e.memoizedState = n);
}
function cd(e, t) {
  var n = j;
  j = n !== 0 && 4 > n ? n : 4, e(!0);
  var r = Vl.transition;
  Vl.transition = {};
  try {
    e(!1), t();
  } finally {
    j = n, Vl.transition = r;
  }
}
function La() {
  return Ne().memoizedState;
}
function fd(e, t, n) {
  var r = at(e);
  if (n = { lane: r, action: n, hasEagerState: !1, eagerState: null, next: null }, Ma(e)) Ra(t, n);
  else if (n = da(e, t, n, r), n !== null) {
    var l = ue();
    je(n, e, r, l), ja(n, t, r);
  }
}
function dd(e, t, n) {
  var r = at(e), l = { lane: r, action: n, hasEagerState: !1, eagerState: null, next: null };
  if (Ma(e)) Ra(t, l);
  else {
    var i = e.alternate;
    if (e.lanes === 0 && (i === null || i.lanes === 0) && (i = t.lastRenderedReducer, i !== null)) try {
      var o = t.lastRenderedState, u = i(o, n);
      if (l.hasEagerState = !0, l.eagerState = u, Oe(u, o)) {
        var s = t.interleaved;
        s === null ? (l.next = l, mo(t)) : (l.next = s.next, s.next = l), t.interleaved = l;
        return;
      }
    } catch {
    } finally {
    }
    n = da(e, t, l, r), n !== null && (l = ue(), je(n, e, r, l), ja(n, t, r));
  }
}
function Ma(e) {
  var t = e.alternate;
  return e === A || t !== null && t === A;
}
function Ra(e, t) {
  zn = Zr = !0;
  var n = e.pending;
  n === null ? t.next = t : (t.next = n.next, n.next = t), e.pending = t;
}
function ja(e, t, n) {
  if (n & 4194240) {
    var r = t.lanes;
    r &= e.pendingLanes, n |= r, t.lanes = n, bi(e, n);
  }
}
var Jr = { readContext: Ce, useCallback: ne, useContext: ne, useEffect: ne, useImperativeHandle: ne, useInsertionEffect: ne, useLayoutEffect: ne, useMemo: ne, useReducer: ne, useRef: ne, useState: ne, useDebugValue: ne, useDeferredValue: ne, useTransition: ne, useMutableSource: ne, useSyncExternalStore: ne, useId: ne, unstable_isNewReconciler: !1 }, pd = { readContext: Ce, useCallback: function(e, t) {
  return Ie().memoizedState = [e, t === void 0 ? null : t], e;
}, useContext: Ce, useEffect: Cu, useImperativeHandle: function(e, t, n) {
  return n = n != null ? n.concat([e]) : null, Pr(
    4194308,
    4,
    Ca.bind(null, t, e),
    n
  );
}, useLayoutEffect: function(e, t) {
  return Pr(4194308, 4, e, t);
}, useInsertionEffect: function(e, t) {
  return Pr(4, 2, e, t);
}, useMemo: function(e, t) {
  var n = Ie();
  return t = t === void 0 ? null : t, e = e(), n.memoizedState = [e, t], e;
}, useReducer: function(e, t, n) {
  var r = Ie();
  return t = n !== void 0 ? n(t) : t, r.memoizedState = r.baseState = t, e = { pending: null, interleaved: null, lanes: 0, dispatch: null, lastRenderedReducer: e, lastRenderedState: t }, r.queue = e, e = e.dispatch = fd.bind(null, A, e), [r.memoizedState, e];
}, useRef: function(e) {
  var t = Ie();
  return e = { current: e }, t.memoizedState = e;
}, useState: _u, useDebugValue: Eo, useDeferredValue: function(e) {
  return Ie().memoizedState = e;
}, useTransition: function() {
  var e = _u(!1), t = e[0];
  return e = cd.bind(null, e[1]), Ie().memoizedState = e, [t, e];
}, useMutableSource: function() {
}, useSyncExternalStore: function(e, t, n) {
  var r = A, l = Ie();
  if ($) {
    if (n === void 0) throw Error(y(407));
    n = n();
  } else {
    if (n = t(), q === null) throw Error(y(349));
    zt & 30 || ya(r, t, n);
  }
  l.memoizedState = n;
  var i = { value: n, getSnapshot: t };
  return l.queue = i, Cu(wa.bind(
    null,
    r,
    i,
    e
  ), [e]), r.flags |= 2048, Kn(9, ga.bind(null, r, i, n, t), void 0, null), n;
}, useId: function() {
  var e = Ie(), t = q.identifierPrefix;
  if ($) {
    var n = He, r = Be;
    n = (r & ~(1 << 32 - Re(r) - 1)).toString(32) + n, t = ":" + t + "R" + n, n = Qn++, 0 < n && (t += "H" + n.toString(32)), t += ":";
  } else n = ad++, t = ":" + t + "r" + n.toString(32) + ":";
  return e.memoizedState = t;
}, unstable_isNewReconciler: !1 }, md = {
  readContext: Ce,
  useCallback: za,
  useContext: Ce,
  useEffect: xo,
  useImperativeHandle: Na,
  useInsertionEffect: Ea,
  useLayoutEffect: _a,
  useMemo: Pa,
  useReducer: Bl,
  useRef: xa,
  useState: function() {
    return Bl(Yn);
  },
  useDebugValue: Eo,
  useDeferredValue: function(e) {
    var t = Ne();
    return Ta(t, K.memoizedState, e);
  },
  useTransition: function() {
    var e = Bl(Yn)[0], t = Ne().memoizedState;
    return [e, t];
  },
  useMutableSource: ha,
  useSyncExternalStore: va,
  useId: La,
  unstable_isNewReconciler: !1
}, hd = { readContext: Ce, useCallback: za, useContext: Ce, useEffect: xo, useImperativeHandle: Na, useInsertionEffect: Ea, useLayoutEffect: _a, useMemo: Pa, useReducer: Hl, useRef: xa, useState: function() {
  return Hl(Yn);
}, useDebugValue: Eo, useDeferredValue: function(e) {
  var t = Ne();
  return K === null ? t.memoizedState = e : Ta(t, K.memoizedState, e);
}, useTransition: function() {
  var e = Hl(Yn)[0], t = Ne().memoizedState;
  return [e, t];
}, useMutableSource: ha, useSyncExternalStore: va, useId: La, unstable_isNewReconciler: !1 };
function Te(e, t) {
  if (e && e.defaultProps) {
    t = V({}, t), e = e.defaultProps;
    for (var n in e) t[n] === void 0 && (t[n] = e[n]);
    return t;
  }
  return t;
}
function Ci(e, t, n, r) {
  t = e.memoizedState, n = n(r, t), n = n == null ? t : V({}, t, n), e.memoizedState = n, e.lanes === 0 && (e.updateQueue.baseState = n);
}
var fl = { isMounted: function(e) {
  return (e = e._reactInternals) ? Mt(e) === e : !1;
}, enqueueSetState: function(e, t, n) {
  e = e._reactInternals;
  var r = ue(), l = at(e), i = We(r, l);
  i.payload = t, n != null && (i.callback = n), t = ut(e, i, l), t !== null && (je(t, e, l, r), Nr(t, e, l));
}, enqueueReplaceState: function(e, t, n) {
  e = e._reactInternals;
  var r = ue(), l = at(e), i = We(r, l);
  i.tag = 1, i.payload = t, n != null && (i.callback = n), t = ut(e, i, l), t !== null && (je(t, e, l, r), Nr(t, e, l));
}, enqueueForceUpdate: function(e, t) {
  e = e._reactInternals;
  var n = ue(), r = at(e), l = We(n, r);
  l.tag = 2, t != null && (l.callback = t), t = ut(e, l, r), t !== null && (je(t, e, r, n), Nr(t, e, r));
} };
function Nu(e, t, n, r, l, i, o) {
  return e = e.stateNode, typeof e.shouldComponentUpdate == "function" ? e.shouldComponentUpdate(r, i, o) : t.prototype && t.prototype.isPureReactComponent ? !Un(n, r) || !Un(l, i) : !0;
}
function Oa(e, t, n) {
  var r = !1, l = dt, i = t.contextType;
  return typeof i == "object" && i !== null ? i = Ce(i) : (l = pe(t) ? Ct : ie.current, r = t.contextTypes, i = (r = r != null) ? qt(e, l) : dt), t = new t(n, i), e.memoizedState = t.state !== null && t.state !== void 0 ? t.state : null, t.updater = fl, e.stateNode = t, t._reactInternals = e, r && (e = e.stateNode, e.__reactInternalMemoizedUnmaskedChildContext = l, e.__reactInternalMemoizedMaskedChildContext = i), t;
}
function zu(e, t, n, r) {
  e = t.state, typeof t.componentWillReceiveProps == "function" && t.componentWillReceiveProps(n, r), typeof t.UNSAFE_componentWillReceiveProps == "function" && t.UNSAFE_componentWillReceiveProps(n, r), t.state !== e && fl.enqueueReplaceState(t, t.state, null);
}
function Ni(e, t, n, r) {
  var l = e.stateNode;
  l.props = n, l.state = e.memoizedState, l.refs = {}, ho(e);
  var i = t.contextType;
  typeof i == "object" && i !== null ? l.context = Ce(i) : (i = pe(t) ? Ct : ie.current, l.context = qt(e, i)), l.state = e.memoizedState, i = t.getDerivedStateFromProps, typeof i == "function" && (Ci(e, t, i, n), l.state = e.memoizedState), typeof t.getDerivedStateFromProps == "function" || typeof l.getSnapshotBeforeUpdate == "function" || typeof l.UNSAFE_componentWillMount != "function" && typeof l.componentWillMount != "function" || (t = l.state, typeof l.componentWillMount == "function" && l.componentWillMount(), typeof l.UNSAFE_componentWillMount == "function" && l.UNSAFE_componentWillMount(), t !== l.state && fl.enqueueReplaceState(l, l.state, null), Xr(e, n, l, r), l.state = e.memoizedState), typeof l.componentDidMount == "function" && (e.flags |= 4194308);
}
function nn(e, t) {
  try {
    var n = "", r = t;
    do
      n += Bc(r), r = r.return;
    while (r);
    var l = n;
  } catch (i) {
    l = `
Error generating stack: ` + i.message + `
` + i.stack;
  }
  return { value: e, source: t, stack: l, digest: null };
}
function Wl(e, t, n) {
  return { value: e, source: null, stack: n ?? null, digest: t ?? null };
}
function zi(e, t) {
  try {
    console.error(t.value);
  } catch (n) {
    setTimeout(function() {
      throw n;
    });
  }
}
var vd = typeof WeakMap == "function" ? WeakMap : Map;
function Da(e, t, n) {
  n = We(-1, n), n.tag = 3, n.payload = { element: null };
  var r = t.value;
  return n.callback = function() {
    br || (br = !0, Fi = r), zi(e, t);
  }, n;
}
function Ia(e, t, n) {
  n = We(-1, n), n.tag = 3;
  var r = e.type.getDerivedStateFromError;
  if (typeof r == "function") {
    var l = t.value;
    n.payload = function() {
      return r(l);
    }, n.callback = function() {
      zi(e, t);
    };
  }
  var i = e.stateNode;
  return i !== null && typeof i.componentDidCatch == "function" && (n.callback = function() {
    zi(e, t), typeof r != "function" && (st === null ? st = /* @__PURE__ */ new Set([this]) : st.add(this));
    var o = t.stack;
    this.componentDidCatch(t.value, { componentStack: o !== null ? o : "" });
  }), n;
}
function Pu(e, t, n) {
  var r = e.pingCache;
  if (r === null) {
    r = e.pingCache = new vd();
    var l = /* @__PURE__ */ new Set();
    r.set(t, l);
  } else l = r.get(t), l === void 0 && (l = /* @__PURE__ */ new Set(), r.set(t, l));
  l.has(n) || (l.add(n), e = Ld.bind(null, e, t, n), t.then(e, e));
}
function Tu(e) {
  do {
    var t;
    if ((t = e.tag === 13) && (t = e.memoizedState, t = t !== null ? t.dehydrated !== null : !0), t) return e;
    e = e.return;
  } while (e !== null);
  return null;
}
function Lu(e, t, n, r, l) {
  return e.mode & 1 ? (e.flags |= 65536, e.lanes = l, e) : (e === t ? e.flags |= 65536 : (e.flags |= 128, n.flags |= 131072, n.flags &= -52805, n.tag === 1 && (n.alternate === null ? n.tag = 17 : (t = We(-1, 1), t.tag = 2, ut(n, t, 1))), n.lanes |= 1), e);
}
var yd = Ge.ReactCurrentOwner, fe = !1;
function oe(e, t, n, r) {
  t.child = e === null ? fa(t, null, n, r) : en(t, e.child, n, r);
}
function Mu(e, t, n, r, l) {
  n = n.render;
  var i = t.ref;
  return Gt(t, l), r = ko(e, t, n, r, i, l), n = So(), e !== null && !fe ? (t.updateQueue = e.updateQueue, t.flags &= -2053, e.lanes &= ~l, Xe(e, t, l)) : ($ && n && uo(t), t.flags |= 1, oe(e, t, r, l), t.child);
}
function Ru(e, t, n, r, l) {
  if (e === null) {
    var i = n.type;
    return typeof i == "function" && !Mo(i) && i.defaultProps === void 0 && n.compare === null && n.defaultProps === void 0 ? (t.tag = 15, t.type = i, Fa(e, t, i, r, l)) : (e = Rr(n.type, null, r, t, t.mode, l), e.ref = t.ref, e.return = t, t.child = e);
  }
  if (i = e.child, !(e.lanes & l)) {
    var o = i.memoizedProps;
    if (n = n.compare, n = n !== null ? n : Un, n(o, r) && e.ref === t.ref) return Xe(e, t, l);
  }
  return t.flags |= 1, e = ct(i, r), e.ref = t.ref, e.return = t, t.child = e;
}
function Fa(e, t, n, r, l) {
  if (e !== null) {
    var i = e.memoizedProps;
    if (Un(i, r) && e.ref === t.ref) if (fe = !1, t.pendingProps = r = i, (e.lanes & l) !== 0) e.flags & 131072 && (fe = !0);
    else return t.lanes = e.lanes, Xe(e, t, l);
  }
  return Pi(e, t, n, r, l);
}
function $a(e, t, n) {
  var r = t.pendingProps, l = r.children, i = e !== null ? e.memoizedState : null;
  if (r.mode === "hidden") if (!(t.mode & 1)) t.memoizedState = { baseLanes: 0, cachePool: null, transitions: null }, D(Wt, he), he |= n;
  else {
    if (!(n & 1073741824)) return e = i !== null ? i.baseLanes | n : n, t.lanes = t.childLanes = 1073741824, t.memoizedState = { baseLanes: e, cachePool: null, transitions: null }, t.updateQueue = null, D(Wt, he), he |= e, null;
    t.memoizedState = { baseLanes: 0, cachePool: null, transitions: null }, r = i !== null ? i.baseLanes : n, D(Wt, he), he |= r;
  }
  else i !== null ? (r = i.baseLanes | n, t.memoizedState = null) : r = n, D(Wt, he), he |= r;
  return oe(e, t, l, n), t.child;
}
function Ua(e, t) {
  var n = t.ref;
  (e === null && n !== null || e !== null && e.ref !== n) && (t.flags |= 512, t.flags |= 2097152);
}
function Pi(e, t, n, r, l) {
  var i = pe(n) ? Ct : ie.current;
  return i = qt(t, i), Gt(t, l), n = ko(e, t, n, r, i, l), r = So(), e !== null && !fe ? (t.updateQueue = e.updateQueue, t.flags &= -2053, e.lanes &= ~l, Xe(e, t, l)) : ($ && r && uo(t), t.flags |= 1, oe(e, t, n, l), t.child);
}
function ju(e, t, n, r, l) {
  if (pe(n)) {
    var i = !0;
    Hr(t);
  } else i = !1;
  if (Gt(t, l), t.stateNode === null) Tr(e, t), Oa(t, n, r), Ni(t, n, r, l), r = !0;
  else if (e === null) {
    var o = t.stateNode, u = t.memoizedProps;
    o.props = u;
    var s = o.context, c = n.contextType;
    typeof c == "object" && c !== null ? c = Ce(c) : (c = pe(n) ? Ct : ie.current, c = qt(t, c));
    var h = n.getDerivedStateFromProps, m = typeof h == "function" || typeof o.getSnapshotBeforeUpdate == "function";
    m || typeof o.UNSAFE_componentWillReceiveProps != "function" && typeof o.componentWillReceiveProps != "function" || (u !== r || s !== c) && zu(t, o, r, c), qe = !1;
    var p = t.memoizedState;
    o.state = p, Xr(t, r, o, l), s = t.memoizedState, u !== r || p !== s || de.current || qe ? (typeof h == "function" && (Ci(t, n, h, r), s = t.memoizedState), (u = qe || Nu(t, n, u, r, p, s, c)) ? (m || typeof o.UNSAFE_componentWillMount != "function" && typeof o.componentWillMount != "function" || (typeof o.componentWillMount == "function" && o.componentWillMount(), typeof o.UNSAFE_componentWillMount == "function" && o.UNSAFE_componentWillMount()), typeof o.componentDidMount == "function" && (t.flags |= 4194308)) : (typeof o.componentDidMount == "function" && (t.flags |= 4194308), t.memoizedProps = r, t.memoizedState = s), o.props = r, o.state = s, o.context = c, r = u) : (typeof o.componentDidMount == "function" && (t.flags |= 4194308), r = !1);
  } else {
    o = t.stateNode, pa(e, t), u = t.memoizedProps, c = t.type === t.elementType ? u : Te(t.type, u), o.props = c, m = t.pendingProps, p = o.context, s = n.contextType, typeof s == "object" && s !== null ? s = Ce(s) : (s = pe(n) ? Ct : ie.current, s = qt(t, s));
    var g = n.getDerivedStateFromProps;
    (h = typeof g == "function" || typeof o.getSnapshotBeforeUpdate == "function") || typeof o.UNSAFE_componentWillReceiveProps != "function" && typeof o.componentWillReceiveProps != "function" || (u !== m || p !== s) && zu(t, o, r, s), qe = !1, p = t.memoizedState, o.state = p, Xr(t, r, o, l);
    var w = t.memoizedState;
    u !== m || p !== w || de.current || qe ? (typeof g == "function" && (Ci(t, n, g, r), w = t.memoizedState), (c = qe || Nu(t, n, c, r, p, w, s) || !1) ? (h || typeof o.UNSAFE_componentWillUpdate != "function" && typeof o.componentWillUpdate != "function" || (typeof o.componentWillUpdate == "function" && o.componentWillUpdate(r, w, s), typeof o.UNSAFE_componentWillUpdate == "function" && o.UNSAFE_componentWillUpdate(r, w, s)), typeof o.componentDidUpdate == "function" && (t.flags |= 4), typeof o.getSnapshotBeforeUpdate == "function" && (t.flags |= 1024)) : (typeof o.componentDidUpdate != "function" || u === e.memoizedProps && p === e.memoizedState || (t.flags |= 4), typeof o.getSnapshotBeforeUpdate != "function" || u === e.memoizedProps && p === e.memoizedState || (t.flags |= 1024), t.memoizedProps = r, t.memoizedState = w), o.props = r, o.state = w, o.context = s, r = c) : (typeof o.componentDidUpdate != "function" || u === e.memoizedProps && p === e.memoizedState || (t.flags |= 4), typeof o.getSnapshotBeforeUpdate != "function" || u === e.memoizedProps && p === e.memoizedState || (t.flags |= 1024), r = !1);
  }
  return Ti(e, t, n, r, i, l);
}
function Ti(e, t, n, r, l, i) {
  Ua(e, t);
  var o = (t.flags & 128) !== 0;
  if (!r && !o) return l && gu(t, n, !1), Xe(e, t, i);
  r = t.stateNode, yd.current = t;
  var u = o && typeof n.getDerivedStateFromError != "function" ? null : r.render();
  return t.flags |= 1, e !== null && o ? (t.child = en(t, e.child, null, i), t.child = en(t, null, u, i)) : oe(e, t, u, i), t.memoizedState = r.state, l && gu(t, n, !0), t.child;
}
function Aa(e) {
  var t = e.stateNode;
  t.pendingContext ? yu(e, t.pendingContext, t.pendingContext !== t.context) : t.context && yu(e, t.context, !1), vo(e, t.containerInfo);
}
function Ou(e, t, n, r, l) {
  return bt(), ao(l), t.flags |= 256, oe(e, t, n, r), t.child;
}
var Li = { dehydrated: null, treeContext: null, retryLane: 0 };
function Mi(e) {
  return { baseLanes: e, cachePool: null, transitions: null };
}
function Va(e, t, n) {
  var r = t.pendingProps, l = U.current, i = !1, o = (t.flags & 128) !== 0, u;
  if ((u = o) || (u = e !== null && e.memoizedState === null ? !1 : (l & 2) !== 0), u ? (i = !0, t.flags &= -129) : (e === null || e.memoizedState !== null) && (l |= 1), D(U, l & 1), e === null)
    return Ei(t), e = t.memoizedState, e !== null && (e = e.dehydrated, e !== null) ? (t.mode & 1 ? e.data === "$!" ? t.lanes = 8 : t.lanes = 1073741824 : t.lanes = 1, null) : (o = r.children, e = r.fallback, i ? (r = t.mode, i = t.child, o = { mode: "hidden", children: o }, !(r & 1) && i !== null ? (i.childLanes = 0, i.pendingProps = o) : i = ml(o, r, 0, null), e = _t(e, r, n, null), i.return = t, e.return = t, i.sibling = e, t.child = i, t.child.memoizedState = Mi(n), t.memoizedState = Li, e) : _o(t, o));
  if (l = e.memoizedState, l !== null && (u = l.dehydrated, u !== null)) return gd(e, t, o, r, u, l, n);
  if (i) {
    i = r.fallback, o = t.mode, l = e.child, u = l.sibling;
    var s = { mode: "hidden", children: r.children };
    return !(o & 1) && t.child !== l ? (r = t.child, r.childLanes = 0, r.pendingProps = s, t.deletions = null) : (r = ct(l, s), r.subtreeFlags = l.subtreeFlags & 14680064), u !== null ? i = ct(u, i) : (i = _t(i, o, n, null), i.flags |= 2), i.return = t, r.return = t, r.sibling = i, t.child = r, r = i, i = t.child, o = e.child.memoizedState, o = o === null ? Mi(n) : { baseLanes: o.baseLanes | n, cachePool: null, transitions: o.transitions }, i.memoizedState = o, i.childLanes = e.childLanes & ~n, t.memoizedState = Li, r;
  }
  return i = e.child, e = i.sibling, r = ct(i, { mode: "visible", children: r.children }), !(t.mode & 1) && (r.lanes = n), r.return = t, r.sibling = null, e !== null && (n = t.deletions, n === null ? (t.deletions = [e], t.flags |= 16) : n.push(e)), t.child = r, t.memoizedState = null, r;
}
function _o(e, t) {
  return t = ml({ mode: "visible", children: t }, e.mode, 0, null), t.return = e, e.child = t;
}
function vr(e, t, n, r) {
  return r !== null && ao(r), en(t, e.child, null, n), e = _o(t, t.pendingProps.children), e.flags |= 2, t.memoizedState = null, e;
}
function gd(e, t, n, r, l, i, o) {
  if (n)
    return t.flags & 256 ? (t.flags &= -257, r = Wl(Error(y(422))), vr(e, t, o, r)) : t.memoizedState !== null ? (t.child = e.child, t.flags |= 128, null) : (i = r.fallback, l = t.mode, r = ml({ mode: "visible", children: r.children }, l, 0, null), i = _t(i, l, o, null), i.flags |= 2, r.return = t, i.return = t, r.sibling = i, t.child = r, t.mode & 1 && en(t, e.child, null, o), t.child.memoizedState = Mi(o), t.memoizedState = Li, i);
  if (!(t.mode & 1)) return vr(e, t, o, null);
  if (l.data === "$!") {
    if (r = l.nextSibling && l.nextSibling.dataset, r) var u = r.dgst;
    return r = u, i = Error(y(419)), r = Wl(i, r, void 0), vr(e, t, o, r);
  }
  if (u = (o & e.childLanes) !== 0, fe || u) {
    if (r = q, r !== null) {
      switch (o & -o) {
        case 4:
          l = 2;
          break;
        case 16:
          l = 8;
          break;
        case 64:
        case 128:
        case 256:
        case 512:
        case 1024:
        case 2048:
        case 4096:
        case 8192:
        case 16384:
        case 32768:
        case 65536:
        case 131072:
        case 262144:
        case 524288:
        case 1048576:
        case 2097152:
        case 4194304:
        case 8388608:
        case 16777216:
        case 33554432:
        case 67108864:
          l = 32;
          break;
        case 536870912:
          l = 268435456;
          break;
        default:
          l = 0;
      }
      l = l & (r.suspendedLanes | o) ? 0 : l, l !== 0 && l !== i.retryLane && (i.retryLane = l, Ke(e, l), je(r, e, l, -1));
    }
    return Lo(), r = Wl(Error(y(421))), vr(e, t, o, r);
  }
  return l.data === "$?" ? (t.flags |= 128, t.child = e.child, t = Md.bind(null, e), l._reactRetry = t, null) : (e = i.treeContext, ve = ot(l.nextSibling), ye = t, $ = !0, Me = null, e !== null && (Se[xe++] = Be, Se[xe++] = He, Se[xe++] = Nt, Be = e.id, He = e.overflow, Nt = t), t = _o(t, r.children), t.flags |= 4096, t);
}
function Du(e, t, n) {
  e.lanes |= t;
  var r = e.alternate;
  r !== null && (r.lanes |= t), _i(e.return, t, n);
}
function Ql(e, t, n, r, l) {
  var i = e.memoizedState;
  i === null ? e.memoizedState = { isBackwards: t, rendering: null, renderingStartTime: 0, last: r, tail: n, tailMode: l } : (i.isBackwards = t, i.rendering = null, i.renderingStartTime = 0, i.last = r, i.tail = n, i.tailMode = l);
}
function Ba(e, t, n) {
  var r = t.pendingProps, l = r.revealOrder, i = r.tail;
  if (oe(e, t, r.children, n), r = U.current, r & 2) r = r & 1 | 2, t.flags |= 128;
  else {
    if (e !== null && e.flags & 128) e: for (e = t.child; e !== null; ) {
      if (e.tag === 13) e.memoizedState !== null && Du(e, n, t);
      else if (e.tag === 19) Du(e, n, t);
      else if (e.child !== null) {
        e.child.return = e, e = e.child;
        continue;
      }
      if (e === t) break e;
      for (; e.sibling === null; ) {
        if (e.return === null || e.return === t) break e;
        e = e.return;
      }
      e.sibling.return = e.return, e = e.sibling;
    }
    r &= 1;
  }
  if (D(U, r), !(t.mode & 1)) t.memoizedState = null;
  else switch (l) {
    case "forwards":
      for (n = t.child, l = null; n !== null; ) e = n.alternate, e !== null && Gr(e) === null && (l = n), n = n.sibling;
      n = l, n === null ? (l = t.child, t.child = null) : (l = n.sibling, n.sibling = null), Ql(t, !1, l, n, i);
      break;
    case "backwards":
      for (n = null, l = t.child, t.child = null; l !== null; ) {
        if (e = l.alternate, e !== null && Gr(e) === null) {
          t.child = l;
          break;
        }
        e = l.sibling, l.sibling = n, n = l, l = e;
      }
      Ql(t, !0, n, null, i);
      break;
    case "together":
      Ql(t, !1, null, null, void 0);
      break;
    default:
      t.memoizedState = null;
  }
  return t.child;
}
function Tr(e, t) {
  !(t.mode & 1) && e !== null && (e.alternate = null, t.alternate = null, t.flags |= 2);
}
function Xe(e, t, n) {
  if (e !== null && (t.dependencies = e.dependencies), Pt |= t.lanes, !(n & t.childLanes)) return null;
  if (e !== null && t.child !== e.child) throw Error(y(153));
  if (t.child !== null) {
    for (e = t.child, n = ct(e, e.pendingProps), t.child = n, n.return = t; e.sibling !== null; ) e = e.sibling, n = n.sibling = ct(e, e.pendingProps), n.return = t;
    n.sibling = null;
  }
  return t.child;
}
function wd(e, t, n) {
  switch (t.tag) {
    case 3:
      Aa(t), bt();
      break;
    case 5:
      ma(t);
      break;
    case 1:
      pe(t.type) && Hr(t);
      break;
    case 4:
      vo(t, t.stateNode.containerInfo);
      break;
    case 10:
      var r = t.type._context, l = t.memoizedProps.value;
      D(Yr, r._currentValue), r._currentValue = l;
      break;
    case 13:
      if (r = t.memoizedState, r !== null)
        return r.dehydrated !== null ? (D(U, U.current & 1), t.flags |= 128, null) : n & t.child.childLanes ? Va(e, t, n) : (D(U, U.current & 1), e = Xe(e, t, n), e !== null ? e.sibling : null);
      D(U, U.current & 1);
      break;
    case 19:
      if (r = (n & t.childLanes) !== 0, e.flags & 128) {
        if (r) return Ba(e, t, n);
        t.flags |= 128;
      }
      if (l = t.memoizedState, l !== null && (l.rendering = null, l.tail = null, l.lastEffect = null), D(U, U.current), r) break;
      return null;
    case 22:
    case 23:
      return t.lanes = 0, $a(e, t, n);
  }
  return Xe(e, t, n);
}
var Ha, Ri, Wa, Qa;
Ha = function(e, t) {
  for (var n = t.child; n !== null; ) {
    if (n.tag === 5 || n.tag === 6) e.appendChild(n.stateNode);
    else if (n.tag !== 4 && n.child !== null) {
      n.child.return = n, n = n.child;
      continue;
    }
    if (n === t) break;
    for (; n.sibling === null; ) {
      if (n.return === null || n.return === t) return;
      n = n.return;
    }
    n.sibling.return = n.return, n = n.sibling;
  }
};
Ri = function() {
};
Wa = function(e, t, n, r) {
  var l = e.memoizedProps;
  if (l !== r) {
    e = t.stateNode, xt(Ue.current);
    var i = null;
    switch (n) {
      case "input":
        l = ei(e, l), r = ei(e, r), i = [];
        break;
      case "select":
        l = V({}, l, { value: void 0 }), r = V({}, r, { value: void 0 }), i = [];
        break;
      case "textarea":
        l = ri(e, l), r = ri(e, r), i = [];
        break;
      default:
        typeof l.onClick != "function" && typeof r.onClick == "function" && (e.onclick = Vr);
    }
    ii(n, r);
    var o;
    n = null;
    for (c in l) if (!r.hasOwnProperty(c) && l.hasOwnProperty(c) && l[c] != null) if (c === "style") {
      var u = l[c];
      for (o in u) u.hasOwnProperty(o) && (n || (n = {}), n[o] = "");
    } else c !== "dangerouslySetInnerHTML" && c !== "children" && c !== "suppressContentEditableWarning" && c !== "suppressHydrationWarning" && c !== "autoFocus" && (Rn.hasOwnProperty(c) ? i || (i = []) : (i = i || []).push(c, null));
    for (c in r) {
      var s = r[c];
      if (u = l != null ? l[c] : void 0, r.hasOwnProperty(c) && s !== u && (s != null || u != null)) if (c === "style") if (u) {
        for (o in u) !u.hasOwnProperty(o) || s && s.hasOwnProperty(o) || (n || (n = {}), n[o] = "");
        for (o in s) s.hasOwnProperty(o) && u[o] !== s[o] && (n || (n = {}), n[o] = s[o]);
      } else n || (i || (i = []), i.push(
        c,
        n
      )), n = s;
      else c === "dangerouslySetInnerHTML" ? (s = s ? s.__html : void 0, u = u ? u.__html : void 0, s != null && u !== s && (i = i || []).push(c, s)) : c === "children" ? typeof s != "string" && typeof s != "number" || (i = i || []).push(c, "" + s) : c !== "suppressContentEditableWarning" && c !== "suppressHydrationWarning" && (Rn.hasOwnProperty(c) ? (s != null && c === "onScroll" && I("scroll", e), i || u === s || (i = [])) : (i = i || []).push(c, s));
    }
    n && (i = i || []).push("style", n);
    var c = i;
    (t.updateQueue = c) && (t.flags |= 4);
  }
};
Qa = function(e, t, n, r) {
  n !== r && (t.flags |= 4);
};
function vn(e, t) {
  if (!$) switch (e.tailMode) {
    case "hidden":
      t = e.tail;
      for (var n = null; t !== null; ) t.alternate !== null && (n = t), t = t.sibling;
      n === null ? e.tail = null : n.sibling = null;
      break;
    case "collapsed":
      n = e.tail;
      for (var r = null; n !== null; ) n.alternate !== null && (r = n), n = n.sibling;
      r === null ? t || e.tail === null ? e.tail = null : e.tail.sibling = null : r.sibling = null;
  }
}
function re(e) {
  var t = e.alternate !== null && e.alternate.child === e.child, n = 0, r = 0;
  if (t) for (var l = e.child; l !== null; ) n |= l.lanes | l.childLanes, r |= l.subtreeFlags & 14680064, r |= l.flags & 14680064, l.return = e, l = l.sibling;
  else for (l = e.child; l !== null; ) n |= l.lanes | l.childLanes, r |= l.subtreeFlags, r |= l.flags, l.return = e, l = l.sibling;
  return e.subtreeFlags |= r, e.childLanes = n, t;
}
function kd(e, t, n) {
  var r = t.pendingProps;
  switch (so(t), t.tag) {
    case 2:
    case 16:
    case 15:
    case 0:
    case 11:
    case 7:
    case 8:
    case 12:
    case 9:
    case 14:
      return re(t), null;
    case 1:
      return pe(t.type) && Br(), re(t), null;
    case 3:
      return r = t.stateNode, tn(), F(de), F(ie), go(), r.pendingContext && (r.context = r.pendingContext, r.pendingContext = null), (e === null || e.child === null) && (mr(t) ? t.flags |= 4 : e === null || e.memoizedState.isDehydrated && !(t.flags & 256) || (t.flags |= 1024, Me !== null && (Ai(Me), Me = null))), Ri(e, t), re(t), null;
    case 5:
      yo(t);
      var l = xt(Wn.current);
      if (n = t.type, e !== null && t.stateNode != null) Wa(e, t, n, r, l), e.ref !== t.ref && (t.flags |= 512, t.flags |= 2097152);
      else {
        if (!r) {
          if (t.stateNode === null) throw Error(y(166));
          return re(t), null;
        }
        if (e = xt(Ue.current), mr(t)) {
          r = t.stateNode, n = t.type;
          var i = t.memoizedProps;
          switch (r[Fe] = t, r[Bn] = i, e = (t.mode & 1) !== 0, n) {
            case "dialog":
              I("cancel", r), I("close", r);
              break;
            case "iframe":
            case "object":
            case "embed":
              I("load", r);
              break;
            case "video":
            case "audio":
              for (l = 0; l < Sn.length; l++) I(Sn[l], r);
              break;
            case "source":
              I("error", r);
              break;
            case "img":
            case "image":
            case "link":
              I(
                "error",
                r
              ), I("load", r);
              break;
            case "details":
              I("toggle", r);
              break;
            case "input":
              Wo(r, i), I("invalid", r);
              break;
            case "select":
              r._wrapperState = { wasMultiple: !!i.multiple }, I("invalid", r);
              break;
            case "textarea":
              Yo(r, i), I("invalid", r);
          }
          ii(n, i), l = null;
          for (var o in i) if (i.hasOwnProperty(o)) {
            var u = i[o];
            o === "children" ? typeof u == "string" ? r.textContent !== u && (i.suppressHydrationWarning !== !0 && pr(r.textContent, u, e), l = ["children", u]) : typeof u == "number" && r.textContent !== "" + u && (i.suppressHydrationWarning !== !0 && pr(
              r.textContent,
              u,
              e
            ), l = ["children", "" + u]) : Rn.hasOwnProperty(o) && u != null && o === "onScroll" && I("scroll", r);
          }
          switch (n) {
            case "input":
              ir(r), Qo(r, i, !0);
              break;
            case "textarea":
              ir(r), Ko(r);
              break;
            case "select":
            case "option":
              break;
            default:
              typeof i.onClick == "function" && (r.onclick = Vr);
          }
          r = l, t.updateQueue = r, r !== null && (t.flags |= 4);
        } else {
          o = l.nodeType === 9 ? l : l.ownerDocument, e === "http://www.w3.org/1999/xhtml" && (e = ws(n)), e === "http://www.w3.org/1999/xhtml" ? n === "script" ? (e = o.createElement("div"), e.innerHTML = "<script><\/script>", e = e.removeChild(e.firstChild)) : typeof r.is == "string" ? e = o.createElement(n, { is: r.is }) : (e = o.createElement(n), n === "select" && (o = e, r.multiple ? o.multiple = !0 : r.size && (o.size = r.size))) : e = o.createElementNS(e, n), e[Fe] = t, e[Bn] = r, Ha(e, t, !1, !1), t.stateNode = e;
          e: {
            switch (o = oi(n, r), n) {
              case "dialog":
                I("cancel", e), I("close", e), l = r;
                break;
              case "iframe":
              case "object":
              case "embed":
                I("load", e), l = r;
                break;
              case "video":
              case "audio":
                for (l = 0; l < Sn.length; l++) I(Sn[l], e);
                l = r;
                break;
              case "source":
                I("error", e), l = r;
                break;
              case "img":
              case "image":
              case "link":
                I(
                  "error",
                  e
                ), I("load", e), l = r;
                break;
              case "details":
                I("toggle", e), l = r;
                break;
              case "input":
                Wo(e, r), l = ei(e, r), I("invalid", e);
                break;
              case "option":
                l = r;
                break;
              case "select":
                e._wrapperState = { wasMultiple: !!r.multiple }, l = V({}, r, { value: void 0 }), I("invalid", e);
                break;
              case "textarea":
                Yo(e, r), l = ri(e, r), I("invalid", e);
                break;
              default:
                l = r;
            }
            ii(n, l), u = l;
            for (i in u) if (u.hasOwnProperty(i)) {
              var s = u[i];
              i === "style" ? xs(e, s) : i === "dangerouslySetInnerHTML" ? (s = s ? s.__html : void 0, s != null && ks(e, s)) : i === "children" ? typeof s == "string" ? (n !== "textarea" || s !== "") && jn(e, s) : typeof s == "number" && jn(e, "" + s) : i !== "suppressContentEditableWarning" && i !== "suppressHydrationWarning" && i !== "autoFocus" && (Rn.hasOwnProperty(i) ? s != null && i === "onScroll" && I("scroll", e) : s != null && Ki(e, i, s, o));
            }
            switch (n) {
              case "input":
                ir(e), Qo(e, r, !1);
                break;
              case "textarea":
                ir(e), Ko(e);
                break;
              case "option":
                r.value != null && e.setAttribute("value", "" + ft(r.value));
                break;
              case "select":
                e.multiple = !!r.multiple, i = r.value, i != null ? Qt(e, !!r.multiple, i, !1) : r.defaultValue != null && Qt(
                  e,
                  !!r.multiple,
                  r.defaultValue,
                  !0
                );
                break;
              default:
                typeof l.onClick == "function" && (e.onclick = Vr);
            }
            switch (n) {
              case "button":
              case "input":
              case "select":
              case "textarea":
                r = !!r.autoFocus;
                break e;
              case "img":
                r = !0;
                break e;
              default:
                r = !1;
            }
          }
          r && (t.flags |= 4);
        }
        t.ref !== null && (t.flags |= 512, t.flags |= 2097152);
      }
      return re(t), null;
    case 6:
      if (e && t.stateNode != null) Qa(e, t, e.memoizedProps, r);
      else {
        if (typeof r != "string" && t.stateNode === null) throw Error(y(166));
        if (n = xt(Wn.current), xt(Ue.current), mr(t)) {
          if (r = t.stateNode, n = t.memoizedProps, r[Fe] = t, (i = r.nodeValue !== n) && (e = ye, e !== null)) switch (e.tag) {
            case 3:
              pr(r.nodeValue, n, (e.mode & 1) !== 0);
              break;
            case 5:
              e.memoizedProps.suppressHydrationWarning !== !0 && pr(r.nodeValue, n, (e.mode & 1) !== 0);
          }
          i && (t.flags |= 4);
        } else r = (n.nodeType === 9 ? n : n.ownerDocument).createTextNode(r), r[Fe] = t, t.stateNode = r;
      }
      return re(t), null;
    case 13:
      if (F(U), r = t.memoizedState, e === null || e.memoizedState !== null && e.memoizedState.dehydrated !== null) {
        if ($ && ve !== null && t.mode & 1 && !(t.flags & 128)) aa(), bt(), t.flags |= 98560, i = !1;
        else if (i = mr(t), r !== null && r.dehydrated !== null) {
          if (e === null) {
            if (!i) throw Error(y(318));
            if (i = t.memoizedState, i = i !== null ? i.dehydrated : null, !i) throw Error(y(317));
            i[Fe] = t;
          } else bt(), !(t.flags & 128) && (t.memoizedState = null), t.flags |= 4;
          re(t), i = !1;
        } else Me !== null && (Ai(Me), Me = null), i = !0;
        if (!i) return t.flags & 65536 ? t : null;
      }
      return t.flags & 128 ? (t.lanes = n, t) : (r = r !== null, r !== (e !== null && e.memoizedState !== null) && r && (t.child.flags |= 8192, t.mode & 1 && (e === null || U.current & 1 ? X === 0 && (X = 3) : Lo())), t.updateQueue !== null && (t.flags |= 4), re(t), null);
    case 4:
      return tn(), Ri(e, t), e === null && An(t.stateNode.containerInfo), re(t), null;
    case 10:
      return po(t.type._context), re(t), null;
    case 17:
      return pe(t.type) && Br(), re(t), null;
    case 19:
      if (F(U), i = t.memoizedState, i === null) return re(t), null;
      if (r = (t.flags & 128) !== 0, o = i.rendering, o === null) if (r) vn(i, !1);
      else {
        if (X !== 0 || e !== null && e.flags & 128) for (e = t.child; e !== null; ) {
          if (o = Gr(e), o !== null) {
            for (t.flags |= 128, vn(i, !1), r = o.updateQueue, r !== null && (t.updateQueue = r, t.flags |= 4), t.subtreeFlags = 0, r = n, n = t.child; n !== null; ) i = n, e = r, i.flags &= 14680066, o = i.alternate, o === null ? (i.childLanes = 0, i.lanes = e, i.child = null, i.subtreeFlags = 0, i.memoizedProps = null, i.memoizedState = null, i.updateQueue = null, i.dependencies = null, i.stateNode = null) : (i.childLanes = o.childLanes, i.lanes = o.lanes, i.child = o.child, i.subtreeFlags = 0, i.deletions = null, i.memoizedProps = o.memoizedProps, i.memoizedState = o.memoizedState, i.updateQueue = o.updateQueue, i.type = o.type, e = o.dependencies, i.dependencies = e === null ? null : { lanes: e.lanes, firstContext: e.firstContext }), n = n.sibling;
            return D(U, U.current & 1 | 2), t.child;
          }
          e = e.sibling;
        }
        i.tail !== null && Q() > rn && (t.flags |= 128, r = !0, vn(i, !1), t.lanes = 4194304);
      }
      else {
        if (!r) if (e = Gr(o), e !== null) {
          if (t.flags |= 128, r = !0, n = e.updateQueue, n !== null && (t.updateQueue = n, t.flags |= 4), vn(i, !0), i.tail === null && i.tailMode === "hidden" && !o.alternate && !$) return re(t), null;
        } else 2 * Q() - i.renderingStartTime > rn && n !== 1073741824 && (t.flags |= 128, r = !0, vn(i, !1), t.lanes = 4194304);
        i.isBackwards ? (o.sibling = t.child, t.child = o) : (n = i.last, n !== null ? n.sibling = o : t.child = o, i.last = o);
      }
      return i.tail !== null ? (t = i.tail, i.rendering = t, i.tail = t.sibling, i.renderingStartTime = Q(), t.sibling = null, n = U.current, D(U, r ? n & 1 | 2 : n & 1), t) : (re(t), null);
    case 22:
    case 23:
      return To(), r = t.memoizedState !== null, e !== null && e.memoizedState !== null !== r && (t.flags |= 8192), r && t.mode & 1 ? he & 1073741824 && (re(t), t.subtreeFlags & 6 && (t.flags |= 8192)) : re(t), null;
    case 24:
      return null;
    case 25:
      return null;
  }
  throw Error(y(156, t.tag));
}
function Sd(e, t) {
  switch (so(t), t.tag) {
    case 1:
      return pe(t.type) && Br(), e = t.flags, e & 65536 ? (t.flags = e & -65537 | 128, t) : null;
    case 3:
      return tn(), F(de), F(ie), go(), e = t.flags, e & 65536 && !(e & 128) ? (t.flags = e & -65537 | 128, t) : null;
    case 5:
      return yo(t), null;
    case 13:
      if (F(U), e = t.memoizedState, e !== null && e.dehydrated !== null) {
        if (t.alternate === null) throw Error(y(340));
        bt();
      }
      return e = t.flags, e & 65536 ? (t.flags = e & -65537 | 128, t) : null;
    case 19:
      return F(U), null;
    case 4:
      return tn(), null;
    case 10:
      return po(t.type._context), null;
    case 22:
    case 23:
      return To(), null;
    case 24:
      return null;
    default:
      return null;
  }
}
var yr = !1, le = !1, xd = typeof WeakSet == "function" ? WeakSet : Set, S = null;
function Ht(e, t) {
  var n = e.ref;
  if (n !== null) if (typeof n == "function") try {
    n(null);
  } catch (r) {
    B(e, t, r);
  }
  else n.current = null;
}
function ji(e, t, n) {
  try {
    n();
  } catch (r) {
    B(e, t, r);
  }
}
var Iu = !1;
function Ed(e, t) {
  if (vi = $r, e = Zs(), oo(e)) {
    if ("selectionStart" in e) var n = { start: e.selectionStart, end: e.selectionEnd };
    else e: {
      n = (n = e.ownerDocument) && n.defaultView || window;
      var r = n.getSelection && n.getSelection();
      if (r && r.rangeCount !== 0) {
        n = r.anchorNode;
        var l = r.anchorOffset, i = r.focusNode;
        r = r.focusOffset;
        try {
          n.nodeType, i.nodeType;
        } catch {
          n = null;
          break e;
        }
        var o = 0, u = -1, s = -1, c = 0, h = 0, m = e, p = null;
        t: for (; ; ) {
          for (var g; m !== n || l !== 0 && m.nodeType !== 3 || (u = o + l), m !== i || r !== 0 && m.nodeType !== 3 || (s = o + r), m.nodeType === 3 && (o += m.nodeValue.length), (g = m.firstChild) !== null; )
            p = m, m = g;
          for (; ; ) {
            if (m === e) break t;
            if (p === n && ++c === l && (u = o), p === i && ++h === r && (s = o), (g = m.nextSibling) !== null) break;
            m = p, p = m.parentNode;
          }
          m = g;
        }
        n = u === -1 || s === -1 ? null : { start: u, end: s };
      } else n = null;
    }
    n = n || { start: 0, end: 0 };
  } else n = null;
  for (yi = { focusedElem: e, selectionRange: n }, $r = !1, S = t; S !== null; ) if (t = S, e = t.child, (t.subtreeFlags & 1028) !== 0 && e !== null) e.return = t, S = e;
  else for (; S !== null; ) {
    t = S;
    try {
      var w = t.alternate;
      if (t.flags & 1024) switch (t.tag) {
        case 0:
        case 11:
        case 15:
          break;
        case 1:
          if (w !== null) {
            var k = w.memoizedProps, O = w.memoizedState, d = t.stateNode, a = d.getSnapshotBeforeUpdate(t.elementType === t.type ? k : Te(t.type, k), O);
            d.__reactInternalSnapshotBeforeUpdate = a;
          }
          break;
        case 3:
          var f = t.stateNode.containerInfo;
          f.nodeType === 1 ? f.textContent = "" : f.nodeType === 9 && f.documentElement && f.removeChild(f.documentElement);
          break;
        case 5:
        case 6:
        case 4:
        case 17:
          break;
        default:
          throw Error(y(163));
      }
    } catch (v) {
      B(t, t.return, v);
    }
    if (e = t.sibling, e !== null) {
      e.return = t.return, S = e;
      break;
    }
    S = t.return;
  }
  return w = Iu, Iu = !1, w;
}
function Pn(e, t, n) {
  var r = t.updateQueue;
  if (r = r !== null ? r.lastEffect : null, r !== null) {
    var l = r = r.next;
    do {
      if ((l.tag & e) === e) {
        var i = l.destroy;
        l.destroy = void 0, i !== void 0 && ji(t, n, i);
      }
      l = l.next;
    } while (l !== r);
  }
}
function dl(e, t) {
  if (t = t.updateQueue, t = t !== null ? t.lastEffect : null, t !== null) {
    var n = t = t.next;
    do {
      if ((n.tag & e) === e) {
        var r = n.create;
        n.destroy = r();
      }
      n = n.next;
    } while (n !== t);
  }
}
function Oi(e) {
  var t = e.ref;
  if (t !== null) {
    var n = e.stateNode;
    switch (e.tag) {
      case 5:
        e = n;
        break;
      default:
        e = n;
    }
    typeof t == "function" ? t(e) : t.current = e;
  }
}
function Ya(e) {
  var t = e.alternate;
  t !== null && (e.alternate = null, Ya(t)), e.child = null, e.deletions = null, e.sibling = null, e.tag === 5 && (t = e.stateNode, t !== null && (delete t[Fe], delete t[Bn], delete t[ki], delete t[id], delete t[od])), e.stateNode = null, e.return = null, e.dependencies = null, e.memoizedProps = null, e.memoizedState = null, e.pendingProps = null, e.stateNode = null, e.updateQueue = null;
}
function Ka(e) {
  return e.tag === 5 || e.tag === 3 || e.tag === 4;
}
function Fu(e) {
  e: for (; ; ) {
    for (; e.sibling === null; ) {
      if (e.return === null || Ka(e.return)) return null;
      e = e.return;
    }
    for (e.sibling.return = e.return, e = e.sibling; e.tag !== 5 && e.tag !== 6 && e.tag !== 18; ) {
      if (e.flags & 2 || e.child === null || e.tag === 4) continue e;
      e.child.return = e, e = e.child;
    }
    if (!(e.flags & 2)) return e.stateNode;
  }
}
function Di(e, t, n) {
  var r = e.tag;
  if (r === 5 || r === 6) e = e.stateNode, t ? n.nodeType === 8 ? n.parentNode.insertBefore(e, t) : n.insertBefore(e, t) : (n.nodeType === 8 ? (t = n.parentNode, t.insertBefore(e, n)) : (t = n, t.appendChild(e)), n = n._reactRootContainer, n != null || t.onclick !== null || (t.onclick = Vr));
  else if (r !== 4 && (e = e.child, e !== null)) for (Di(e, t, n), e = e.sibling; e !== null; ) Di(e, t, n), e = e.sibling;
}
function Ii(e, t, n) {
  var r = e.tag;
  if (r === 5 || r === 6) e = e.stateNode, t ? n.insertBefore(e, t) : n.appendChild(e);
  else if (r !== 4 && (e = e.child, e !== null)) for (Ii(e, t, n), e = e.sibling; e !== null; ) Ii(e, t, n), e = e.sibling;
}
var b = null, Le = !1;
function Ze(e, t, n) {
  for (n = n.child; n !== null; ) Xa(e, t, n), n = n.sibling;
}
function Xa(e, t, n) {
  if ($e && typeof $e.onCommitFiberUnmount == "function") try {
    $e.onCommitFiberUnmount(ll, n);
  } catch {
  }
  switch (n.tag) {
    case 5:
      le || Ht(n, t);
    case 6:
      var r = b, l = Le;
      b = null, Ze(e, t, n), b = r, Le = l, b !== null && (Le ? (e = b, n = n.stateNode, e.nodeType === 8 ? e.parentNode.removeChild(n) : e.removeChild(n)) : b.removeChild(n.stateNode));
      break;
    case 18:
      b !== null && (Le ? (e = b, n = n.stateNode, e.nodeType === 8 ? $l(e.parentNode, n) : e.nodeType === 1 && $l(e, n), Fn(e)) : $l(b, n.stateNode));
      break;
    case 4:
      r = b, l = Le, b = n.stateNode.containerInfo, Le = !0, Ze(e, t, n), b = r, Le = l;
      break;
    case 0:
    case 11:
    case 14:
    case 15:
      if (!le && (r = n.updateQueue, r !== null && (r = r.lastEffect, r !== null))) {
        l = r = r.next;
        do {
          var i = l, o = i.destroy;
          i = i.tag, o !== void 0 && (i & 2 || i & 4) && ji(n, t, o), l = l.next;
        } while (l !== r);
      }
      Ze(e, t, n);
      break;
    case 1:
      if (!le && (Ht(n, t), r = n.stateNode, typeof r.componentWillUnmount == "function")) try {
        r.props = n.memoizedProps, r.state = n.memoizedState, r.componentWillUnmount();
      } catch (u) {
        B(n, t, u);
      }
      Ze(e, t, n);
      break;
    case 21:
      Ze(e, t, n);
      break;
    case 22:
      n.mode & 1 ? (le = (r = le) || n.memoizedState !== null, Ze(e, t, n), le = r) : Ze(e, t, n);
      break;
    default:
      Ze(e, t, n);
  }
}
function $u(e) {
  var t = e.updateQueue;
  if (t !== null) {
    e.updateQueue = null;
    var n = e.stateNode;
    n === null && (n = e.stateNode = new xd()), t.forEach(function(r) {
      var l = Rd.bind(null, e, r);
      n.has(r) || (n.add(r), r.then(l, l));
    });
  }
}
function Pe(e, t) {
  var n = t.deletions;
  if (n !== null) for (var r = 0; r < n.length; r++) {
    var l = n[r];
    try {
      var i = e, o = t, u = o;
      e: for (; u !== null; ) {
        switch (u.tag) {
          case 5:
            b = u.stateNode, Le = !1;
            break e;
          case 3:
            b = u.stateNode.containerInfo, Le = !0;
            break e;
          case 4:
            b = u.stateNode.containerInfo, Le = !0;
            break e;
        }
        u = u.return;
      }
      if (b === null) throw Error(y(160));
      Xa(i, o, l), b = null, Le = !1;
      var s = l.alternate;
      s !== null && (s.return = null), l.return = null;
    } catch (c) {
      B(l, t, c);
    }
  }
  if (t.subtreeFlags & 12854) for (t = t.child; t !== null; ) Ga(t, e), t = t.sibling;
}
function Ga(e, t) {
  var n = e.alternate, r = e.flags;
  switch (e.tag) {
    case 0:
    case 11:
    case 14:
    case 15:
      if (Pe(t, e), De(e), r & 4) {
        try {
          Pn(3, e, e.return), dl(3, e);
        } catch (k) {
          B(e, e.return, k);
        }
        try {
          Pn(5, e, e.return);
        } catch (k) {
          B(e, e.return, k);
        }
      }
      break;
    case 1:
      Pe(t, e), De(e), r & 512 && n !== null && Ht(n, n.return);
      break;
    case 5:
      if (Pe(t, e), De(e), r & 512 && n !== null && Ht(n, n.return), e.flags & 32) {
        var l = e.stateNode;
        try {
          jn(l, "");
        } catch (k) {
          B(e, e.return, k);
        }
      }
      if (r & 4 && (l = e.stateNode, l != null)) {
        var i = e.memoizedProps, o = n !== null ? n.memoizedProps : i, u = e.type, s = e.updateQueue;
        if (e.updateQueue = null, s !== null) try {
          u === "input" && i.type === "radio" && i.name != null && ys(l, i), oi(u, o);
          var c = oi(u, i);
          for (o = 0; o < s.length; o += 2) {
            var h = s[o], m = s[o + 1];
            h === "style" ? xs(l, m) : h === "dangerouslySetInnerHTML" ? ks(l, m) : h === "children" ? jn(l, m) : Ki(l, h, m, c);
          }
          switch (u) {
            case "input":
              ti(l, i);
              break;
            case "textarea":
              gs(l, i);
              break;
            case "select":
              var p = l._wrapperState.wasMultiple;
              l._wrapperState.wasMultiple = !!i.multiple;
              var g = i.value;
              g != null ? Qt(l, !!i.multiple, g, !1) : p !== !!i.multiple && (i.defaultValue != null ? Qt(
                l,
                !!i.multiple,
                i.defaultValue,
                !0
              ) : Qt(l, !!i.multiple, i.multiple ? [] : "", !1));
          }
          l[Bn] = i;
        } catch (k) {
          B(e, e.return, k);
        }
      }
      break;
    case 6:
      if (Pe(t, e), De(e), r & 4) {
        if (e.stateNode === null) throw Error(y(162));
        l = e.stateNode, i = e.memoizedProps;
        try {
          l.nodeValue = i;
        } catch (k) {
          B(e, e.return, k);
        }
      }
      break;
    case 3:
      if (Pe(t, e), De(e), r & 4 && n !== null && n.memoizedState.isDehydrated) try {
        Fn(t.containerInfo);
      } catch (k) {
        B(e, e.return, k);
      }
      break;
    case 4:
      Pe(t, e), De(e);
      break;
    case 13:
      Pe(t, e), De(e), l = e.child, l.flags & 8192 && (i = l.memoizedState !== null, l.stateNode.isHidden = i, !i || l.alternate !== null && l.alternate.memoizedState !== null || (zo = Q())), r & 4 && $u(e);
      break;
    case 22:
      if (h = n !== null && n.memoizedState !== null, e.mode & 1 ? (le = (c = le) || h, Pe(t, e), le = c) : Pe(t, e), De(e), r & 8192) {
        if (c = e.memoizedState !== null, (e.stateNode.isHidden = c) && !h && e.mode & 1) for (S = e, h = e.child; h !== null; ) {
          for (m = S = h; S !== null; ) {
            switch (p = S, g = p.child, p.tag) {
              case 0:
              case 11:
              case 14:
              case 15:
                Pn(4, p, p.return);
                break;
              case 1:
                Ht(p, p.return);
                var w = p.stateNode;
                if (typeof w.componentWillUnmount == "function") {
                  r = p, n = p.return;
                  try {
                    t = r, w.props = t.memoizedProps, w.state = t.memoizedState, w.componentWillUnmount();
                  } catch (k) {
                    B(r, n, k);
                  }
                }
                break;
              case 5:
                Ht(p, p.return);
                break;
              case 22:
                if (p.memoizedState !== null) {
                  Au(m);
                  continue;
                }
            }
            g !== null ? (g.return = p, S = g) : Au(m);
          }
          h = h.sibling;
        }
        e: for (h = null, m = e; ; ) {
          if (m.tag === 5) {
            if (h === null) {
              h = m;
              try {
                l = m.stateNode, c ? (i = l.style, typeof i.setProperty == "function" ? i.setProperty("display", "none", "important") : i.display = "none") : (u = m.stateNode, s = m.memoizedProps.style, o = s != null && s.hasOwnProperty("display") ? s.display : null, u.style.display = Ss("display", o));
              } catch (k) {
                B(e, e.return, k);
              }
            }
          } else if (m.tag === 6) {
            if (h === null) try {
              m.stateNode.nodeValue = c ? "" : m.memoizedProps;
            } catch (k) {
              B(e, e.return, k);
            }
          } else if ((m.tag !== 22 && m.tag !== 23 || m.memoizedState === null || m === e) && m.child !== null) {
            m.child.return = m, m = m.child;
            continue;
          }
          if (m === e) break e;
          for (; m.sibling === null; ) {
            if (m.return === null || m.return === e) break e;
            h === m && (h = null), m = m.return;
          }
          h === m && (h = null), m.sibling.return = m.return, m = m.sibling;
        }
      }
      break;
    case 19:
      Pe(t, e), De(e), r & 4 && $u(e);
      break;
    case 21:
      break;
    default:
      Pe(
        t,
        e
      ), De(e);
  }
}
function De(e) {
  var t = e.flags;
  if (t & 2) {
    try {
      e: {
        for (var n = e.return; n !== null; ) {
          if (Ka(n)) {
            var r = n;
            break e;
          }
          n = n.return;
        }
        throw Error(y(160));
      }
      switch (r.tag) {
        case 5:
          var l = r.stateNode;
          r.flags & 32 && (jn(l, ""), r.flags &= -33);
          var i = Fu(e);
          Ii(e, i, l);
          break;
        case 3:
        case 4:
          var o = r.stateNode.containerInfo, u = Fu(e);
          Di(e, u, o);
          break;
        default:
          throw Error(y(161));
      }
    } catch (s) {
      B(e, e.return, s);
    }
    e.flags &= -3;
  }
  t & 4096 && (e.flags &= -4097);
}
function _d(e, t, n) {
  S = e, Za(e);
}
function Za(e, t, n) {
  for (var r = (e.mode & 1) !== 0; S !== null; ) {
    var l = S, i = l.child;
    if (l.tag === 22 && r) {
      var o = l.memoizedState !== null || yr;
      if (!o) {
        var u = l.alternate, s = u !== null && u.memoizedState !== null || le;
        u = yr;
        var c = le;
        if (yr = o, (le = s) && !c) for (S = l; S !== null; ) o = S, s = o.child, o.tag === 22 && o.memoizedState !== null ? Vu(l) : s !== null ? (s.return = o, S = s) : Vu(l);
        for (; i !== null; ) S = i, Za(i), i = i.sibling;
        S = l, yr = u, le = c;
      }
      Uu(e);
    } else l.subtreeFlags & 8772 && i !== null ? (i.return = l, S = i) : Uu(e);
  }
}
function Uu(e) {
  for (; S !== null; ) {
    var t = S;
    if (t.flags & 8772) {
      var n = t.alternate;
      try {
        if (t.flags & 8772) switch (t.tag) {
          case 0:
          case 11:
          case 15:
            le || dl(5, t);
            break;
          case 1:
            var r = t.stateNode;
            if (t.flags & 4 && !le) if (n === null) r.componentDidMount();
            else {
              var l = t.elementType === t.type ? n.memoizedProps : Te(t.type, n.memoizedProps);
              r.componentDidUpdate(l, n.memoizedState, r.__reactInternalSnapshotBeforeUpdate);
            }
            var i = t.updateQueue;
            i !== null && Eu(t, i, r);
            break;
          case 3:
            var o = t.updateQueue;
            if (o !== null) {
              if (n = null, t.child !== null) switch (t.child.tag) {
                case 5:
                  n = t.child.stateNode;
                  break;
                case 1:
                  n = t.child.stateNode;
              }
              Eu(t, o, n);
            }
            break;
          case 5:
            var u = t.stateNode;
            if (n === null && t.flags & 4) {
              n = u;
              var s = t.memoizedProps;
              switch (t.type) {
                case "button":
                case "input":
                case "select":
                case "textarea":
                  s.autoFocus && n.focus();
                  break;
                case "img":
                  s.src && (n.src = s.src);
              }
            }
            break;
          case 6:
            break;
          case 4:
            break;
          case 12:
            break;
          case 13:
            if (t.memoizedState === null) {
              var c = t.alternate;
              if (c !== null) {
                var h = c.memoizedState;
                if (h !== null) {
                  var m = h.dehydrated;
                  m !== null && Fn(m);
                }
              }
            }
            break;
          case 19:
          case 17:
          case 21:
          case 22:
          case 23:
          case 25:
            break;
          default:
            throw Error(y(163));
        }
        le || t.flags & 512 && Oi(t);
      } catch (p) {
        B(t, t.return, p);
      }
    }
    if (t === e) {
      S = null;
      break;
    }
    if (n = t.sibling, n !== null) {
      n.return = t.return, S = n;
      break;
    }
    S = t.return;
  }
}
function Au(e) {
  for (; S !== null; ) {
    var t = S;
    if (t === e) {
      S = null;
      break;
    }
    var n = t.sibling;
    if (n !== null) {
      n.return = t.return, S = n;
      break;
    }
    S = t.return;
  }
}
function Vu(e) {
  for (; S !== null; ) {
    var t = S;
    try {
      switch (t.tag) {
        case 0:
        case 11:
        case 15:
          var n = t.return;
          try {
            dl(4, t);
          } catch (s) {
            B(t, n, s);
          }
          break;
        case 1:
          var r = t.stateNode;
          if (typeof r.componentDidMount == "function") {
            var l = t.return;
            try {
              r.componentDidMount();
            } catch (s) {
              B(t, l, s);
            }
          }
          var i = t.return;
          try {
            Oi(t);
          } catch (s) {
            B(t, i, s);
          }
          break;
        case 5:
          var o = t.return;
          try {
            Oi(t);
          } catch (s) {
            B(t, o, s);
          }
      }
    } catch (s) {
      B(t, t.return, s);
    }
    if (t === e) {
      S = null;
      break;
    }
    var u = t.sibling;
    if (u !== null) {
      u.return = t.return, S = u;
      break;
    }
    S = t.return;
  }
}
var Cd = Math.ceil, qr = Ge.ReactCurrentDispatcher, Co = Ge.ReactCurrentOwner, _e = Ge.ReactCurrentBatchConfig, R = 0, q = null, Y = null, ee = 0, he = 0, Wt = mt(0), X = 0, Xn = null, Pt = 0, pl = 0, No = 0, Tn = null, ce = null, zo = 0, rn = 1 / 0, Ae = null, br = !1, Fi = null, st = null, gr = !1, nt = null, el = 0, Ln = 0, $i = null, Lr = -1, Mr = 0;
function ue() {
  return R & 6 ? Q() : Lr !== -1 ? Lr : Lr = Q();
}
function at(e) {
  return e.mode & 1 ? R & 2 && ee !== 0 ? ee & -ee : sd.transition !== null ? (Mr === 0 && (Mr = Os()), Mr) : (e = j, e !== 0 || (e = window.event, e = e === void 0 ? 16 : Vs(e.type)), e) : 1;
}
function je(e, t, n, r) {
  if (50 < Ln) throw Ln = 0, $i = null, Error(y(185));
  Zn(e, n, r), (!(R & 2) || e !== q) && (e === q && (!(R & 2) && (pl |= n), X === 4 && et(e, ee)), me(e, r), n === 1 && R === 0 && !(t.mode & 1) && (rn = Q() + 500, al && ht()));
}
function me(e, t) {
  var n = e.callbackNode;
  sf(e, t);
  var r = Fr(e, e === q ? ee : 0);
  if (r === 0) n !== null && Zo(n), e.callbackNode = null, e.callbackPriority = 0;
  else if (t = r & -r, e.callbackPriority !== t) {
    if (n != null && Zo(n), t === 1) e.tag === 0 ? ud(Bu.bind(null, e)) : oa(Bu.bind(null, e)), rd(function() {
      !(R & 6) && ht();
    }), n = null;
    else {
      switch (Ds(r)) {
        case 1:
          n = qi;
          break;
        case 4:
          n = Rs;
          break;
        case 16:
          n = Ir;
          break;
        case 536870912:
          n = js;
          break;
        default:
          n = Ir;
      }
      n = lc(n, Ja.bind(null, e));
    }
    e.callbackPriority = t, e.callbackNode = n;
  }
}
function Ja(e, t) {
  if (Lr = -1, Mr = 0, R & 6) throw Error(y(327));
  var n = e.callbackNode;
  if (Zt() && e.callbackNode !== n) return null;
  var r = Fr(e, e === q ? ee : 0);
  if (r === 0) return null;
  if (r & 30 || r & e.expiredLanes || t) t = tl(e, r);
  else {
    t = r;
    var l = R;
    R |= 2;
    var i = ba();
    (q !== e || ee !== t) && (Ae = null, rn = Q() + 500, Et(e, t));
    do
      try {
        Pd();
        break;
      } catch (u) {
        qa(e, u);
      }
    while (!0);
    fo(), qr.current = i, R = l, Y !== null ? t = 0 : (q = null, ee = 0, t = X);
  }
  if (t !== 0) {
    if (t === 2 && (l = fi(e), l !== 0 && (r = l, t = Ui(e, l))), t === 1) throw n = Xn, Et(e, 0), et(e, r), me(e, Q()), n;
    if (t === 6) et(e, r);
    else {
      if (l = e.current.alternate, !(r & 30) && !Nd(l) && (t = tl(e, r), t === 2 && (i = fi(e), i !== 0 && (r = i, t = Ui(e, i))), t === 1)) throw n = Xn, Et(e, 0), et(e, r), me(e, Q()), n;
      switch (e.finishedWork = l, e.finishedLanes = r, t) {
        case 0:
        case 1:
          throw Error(y(345));
        case 2:
          wt(e, ce, Ae);
          break;
        case 3:
          if (et(e, r), (r & 130023424) === r && (t = zo + 500 - Q(), 10 < t)) {
            if (Fr(e, 0) !== 0) break;
            if (l = e.suspendedLanes, (l & r) !== r) {
              ue(), e.pingedLanes |= e.suspendedLanes & l;
              break;
            }
            e.timeoutHandle = wi(wt.bind(null, e, ce, Ae), t);
            break;
          }
          wt(e, ce, Ae);
          break;
        case 4:
          if (et(e, r), (r & 4194240) === r) break;
          for (t = e.eventTimes, l = -1; 0 < r; ) {
            var o = 31 - Re(r);
            i = 1 << o, o = t[o], o > l && (l = o), r &= ~i;
          }
          if (r = l, r = Q() - r, r = (120 > r ? 120 : 480 > r ? 480 : 1080 > r ? 1080 : 1920 > r ? 1920 : 3e3 > r ? 3e3 : 4320 > r ? 4320 : 1960 * Cd(r / 1960)) - r, 10 < r) {
            e.timeoutHandle = wi(wt.bind(null, e, ce, Ae), r);
            break;
          }
          wt(e, ce, Ae);
          break;
        case 5:
          wt(e, ce, Ae);
          break;
        default:
          throw Error(y(329));
      }
    }
  }
  return me(e, Q()), e.callbackNode === n ? Ja.bind(null, e) : null;
}
function Ui(e, t) {
  var n = Tn;
  return e.current.memoizedState.isDehydrated && (Et(e, t).flags |= 256), e = tl(e, t), e !== 2 && (t = ce, ce = n, t !== null && Ai(t)), e;
}
function Ai(e) {
  ce === null ? ce = e : ce.push.apply(ce, e);
}
function Nd(e) {
  for (var t = e; ; ) {
    if (t.flags & 16384) {
      var n = t.updateQueue;
      if (n !== null && (n = n.stores, n !== null)) for (var r = 0; r < n.length; r++) {
        var l = n[r], i = l.getSnapshot;
        l = l.value;
        try {
          if (!Oe(i(), l)) return !1;
        } catch {
          return !1;
        }
      }
    }
    if (n = t.child, t.subtreeFlags & 16384 && n !== null) n.return = t, t = n;
    else {
      if (t === e) break;
      for (; t.sibling === null; ) {
        if (t.return === null || t.return === e) return !0;
        t = t.return;
      }
      t.sibling.return = t.return, t = t.sibling;
    }
  }
  return !0;
}
function et(e, t) {
  for (t &= ~No, t &= ~pl, e.suspendedLanes |= t, e.pingedLanes &= ~t, e = e.expirationTimes; 0 < t; ) {
    var n = 31 - Re(t), r = 1 << n;
    e[n] = -1, t &= ~r;
  }
}
function Bu(e) {
  if (R & 6) throw Error(y(327));
  Zt();
  var t = Fr(e, 0);
  if (!(t & 1)) return me(e, Q()), null;
  var n = tl(e, t);
  if (e.tag !== 0 && n === 2) {
    var r = fi(e);
    r !== 0 && (t = r, n = Ui(e, r));
  }
  if (n === 1) throw n = Xn, Et(e, 0), et(e, t), me(e, Q()), n;
  if (n === 6) throw Error(y(345));
  return e.finishedWork = e.current.alternate, e.finishedLanes = t, wt(e, ce, Ae), me(e, Q()), null;
}
function Po(e, t) {
  var n = R;
  R |= 1;
  try {
    return e(t);
  } finally {
    R = n, R === 0 && (rn = Q() + 500, al && ht());
  }
}
function Tt(e) {
  nt !== null && nt.tag === 0 && !(R & 6) && Zt();
  var t = R;
  R |= 1;
  var n = _e.transition, r = j;
  try {
    if (_e.transition = null, j = 1, e) return e();
  } finally {
    j = r, _e.transition = n, R = t, !(R & 6) && ht();
  }
}
function To() {
  he = Wt.current, F(Wt);
}
function Et(e, t) {
  e.finishedWork = null, e.finishedLanes = 0;
  var n = e.timeoutHandle;
  if (n !== -1 && (e.timeoutHandle = -1, nd(n)), Y !== null) for (n = Y.return; n !== null; ) {
    var r = n;
    switch (so(r), r.tag) {
      case 1:
        r = r.type.childContextTypes, r != null && Br();
        break;
      case 3:
        tn(), F(de), F(ie), go();
        break;
      case 5:
        yo(r);
        break;
      case 4:
        tn();
        break;
      case 13:
        F(U);
        break;
      case 19:
        F(U);
        break;
      case 10:
        po(r.type._context);
        break;
      case 22:
      case 23:
        To();
    }
    n = n.return;
  }
  if (q = e, Y = e = ct(e.current, null), ee = he = t, X = 0, Xn = null, No = pl = Pt = 0, ce = Tn = null, St !== null) {
    for (t = 0; t < St.length; t++) if (n = St[t], r = n.interleaved, r !== null) {
      n.interleaved = null;
      var l = r.next, i = n.pending;
      if (i !== null) {
        var o = i.next;
        i.next = l, r.next = o;
      }
      n.pending = r;
    }
    St = null;
  }
  return e;
}
function qa(e, t) {
  do {
    var n = Y;
    try {
      if (fo(), zr.current = Jr, Zr) {
        for (var r = A.memoizedState; r !== null; ) {
          var l = r.queue;
          l !== null && (l.pending = null), r = r.next;
        }
        Zr = !1;
      }
      if (zt = 0, Z = K = A = null, zn = !1, Qn = 0, Co.current = null, n === null || n.return === null) {
        X = 1, Xn = t, Y = null;
        break;
      }
      e: {
        var i = e, o = n.return, u = n, s = t;
        if (t = ee, u.flags |= 32768, s !== null && typeof s == "object" && typeof s.then == "function") {
          var c = s, h = u, m = h.tag;
          if (!(h.mode & 1) && (m === 0 || m === 11 || m === 15)) {
            var p = h.alternate;
            p ? (h.updateQueue = p.updateQueue, h.memoizedState = p.memoizedState, h.lanes = p.lanes) : (h.updateQueue = null, h.memoizedState = null);
          }
          var g = Tu(o);
          if (g !== null) {
            g.flags &= -257, Lu(g, o, u, i, t), g.mode & 1 && Pu(i, c, t), t = g, s = c;
            var w = t.updateQueue;
            if (w === null) {
              var k = /* @__PURE__ */ new Set();
              k.add(s), t.updateQueue = k;
            } else w.add(s);
            break e;
          } else {
            if (!(t & 1)) {
              Pu(i, c, t), Lo();
              break e;
            }
            s = Error(y(426));
          }
        } else if ($ && u.mode & 1) {
          var O = Tu(o);
          if (O !== null) {
            !(O.flags & 65536) && (O.flags |= 256), Lu(O, o, u, i, t), ao(nn(s, u));
            break e;
          }
        }
        i = s = nn(s, u), X !== 4 && (X = 2), Tn === null ? Tn = [i] : Tn.push(i), i = o;
        do {
          switch (i.tag) {
            case 3:
              i.flags |= 65536, t &= -t, i.lanes |= t;
              var d = Da(i, s, t);
              xu(i, d);
              break e;
            case 1:
              u = s;
              var a = i.type, f = i.stateNode;
              if (!(i.flags & 128) && (typeof a.getDerivedStateFromError == "function" || f !== null && typeof f.componentDidCatch == "function" && (st === null || !st.has(f)))) {
                i.flags |= 65536, t &= -t, i.lanes |= t;
                var v = Ia(i, u, t);
                xu(i, v);
                break e;
              }
          }
          i = i.return;
        } while (i !== null);
      }
      tc(n);
    } catch (x) {
      t = x, Y === n && n !== null && (Y = n = n.return);
      continue;
    }
    break;
  } while (!0);
}
function ba() {
  var e = qr.current;
  return qr.current = Jr, e === null ? Jr : e;
}
function Lo() {
  (X === 0 || X === 3 || X === 2) && (X = 4), q === null || !(Pt & 268435455) && !(pl & 268435455) || et(q, ee);
}
function tl(e, t) {
  var n = R;
  R |= 2;
  var r = ba();
  (q !== e || ee !== t) && (Ae = null, Et(e, t));
  do
    try {
      zd();
      break;
    } catch (l) {
      qa(e, l);
    }
  while (!0);
  if (fo(), R = n, qr.current = r, Y !== null) throw Error(y(261));
  return q = null, ee = 0, X;
}
function zd() {
  for (; Y !== null; ) ec(Y);
}
function Pd() {
  for (; Y !== null && !qc(); ) ec(Y);
}
function ec(e) {
  var t = rc(e.alternate, e, he);
  e.memoizedProps = e.pendingProps, t === null ? tc(e) : Y = t, Co.current = null;
}
function tc(e) {
  var t = e;
  do {
    var n = t.alternate;
    if (e = t.return, t.flags & 32768) {
      if (n = Sd(n, t), n !== null) {
        n.flags &= 32767, Y = n;
        return;
      }
      if (e !== null) e.flags |= 32768, e.subtreeFlags = 0, e.deletions = null;
      else {
        X = 6, Y = null;
        return;
      }
    } else if (n = kd(n, t, he), n !== null) {
      Y = n;
      return;
    }
    if (t = t.sibling, t !== null) {
      Y = t;
      return;
    }
    Y = t = e;
  } while (t !== null);
  X === 0 && (X = 5);
}
function wt(e, t, n) {
  var r = j, l = _e.transition;
  try {
    _e.transition = null, j = 1, Td(e, t, n, r);
  } finally {
    _e.transition = l, j = r;
  }
  return null;
}
function Td(e, t, n, r) {
  do
    Zt();
  while (nt !== null);
  if (R & 6) throw Error(y(327));
  n = e.finishedWork;
  var l = e.finishedLanes;
  if (n === null) return null;
  if (e.finishedWork = null, e.finishedLanes = 0, n === e.current) throw Error(y(177));
  e.callbackNode = null, e.callbackPriority = 0;
  var i = n.lanes | n.childLanes;
  if (af(e, i), e === q && (Y = q = null, ee = 0), !(n.subtreeFlags & 2064) && !(n.flags & 2064) || gr || (gr = !0, lc(Ir, function() {
    return Zt(), null;
  })), i = (n.flags & 15990) !== 0, n.subtreeFlags & 15990 || i) {
    i = _e.transition, _e.transition = null;
    var o = j;
    j = 1;
    var u = R;
    R |= 4, Co.current = null, Ed(e, n), Ga(n, e), Gf(yi), $r = !!vi, yi = vi = null, e.current = n, _d(n), bc(), R = u, j = o, _e.transition = i;
  } else e.current = n;
  if (gr && (gr = !1, nt = e, el = l), i = e.pendingLanes, i === 0 && (st = null), nf(n.stateNode), me(e, Q()), t !== null) for (r = e.onRecoverableError, n = 0; n < t.length; n++) l = t[n], r(l.value, { componentStack: l.stack, digest: l.digest });
  if (br) throw br = !1, e = Fi, Fi = null, e;
  return el & 1 && e.tag !== 0 && Zt(), i = e.pendingLanes, i & 1 ? e === $i ? Ln++ : (Ln = 0, $i = e) : Ln = 0, ht(), null;
}
function Zt() {
  if (nt !== null) {
    var e = Ds(el), t = _e.transition, n = j;
    try {
      if (_e.transition = null, j = 16 > e ? 16 : e, nt === null) var r = !1;
      else {
        if (e = nt, nt = null, el = 0, R & 6) throw Error(y(331));
        var l = R;
        for (R |= 4, S = e.current; S !== null; ) {
          var i = S, o = i.child;
          if (S.flags & 16) {
            var u = i.deletions;
            if (u !== null) {
              for (var s = 0; s < u.length; s++) {
                var c = u[s];
                for (S = c; S !== null; ) {
                  var h = S;
                  switch (h.tag) {
                    case 0:
                    case 11:
                    case 15:
                      Pn(8, h, i);
                  }
                  var m = h.child;
                  if (m !== null) m.return = h, S = m;
                  else for (; S !== null; ) {
                    h = S;
                    var p = h.sibling, g = h.return;
                    if (Ya(h), h === c) {
                      S = null;
                      break;
                    }
                    if (p !== null) {
                      p.return = g, S = p;
                      break;
                    }
                    S = g;
                  }
                }
              }
              var w = i.alternate;
              if (w !== null) {
                var k = w.child;
                if (k !== null) {
                  w.child = null;
                  do {
                    var O = k.sibling;
                    k.sibling = null, k = O;
                  } while (k !== null);
                }
              }
              S = i;
            }
          }
          if (i.subtreeFlags & 2064 && o !== null) o.return = i, S = o;
          else e: for (; S !== null; ) {
            if (i = S, i.flags & 2048) switch (i.tag) {
              case 0:
              case 11:
              case 15:
                Pn(9, i, i.return);
            }
            var d = i.sibling;
            if (d !== null) {
              d.return = i.return, S = d;
              break e;
            }
            S = i.return;
          }
        }
        var a = e.current;
        for (S = a; S !== null; ) {
          o = S;
          var f = o.child;
          if (o.subtreeFlags & 2064 && f !== null) f.return = o, S = f;
          else e: for (o = a; S !== null; ) {
            if (u = S, u.flags & 2048) try {
              switch (u.tag) {
                case 0:
                case 11:
                case 15:
                  dl(9, u);
              }
            } catch (x) {
              B(u, u.return, x);
            }
            if (u === o) {
              S = null;
              break e;
            }
            var v = u.sibling;
            if (v !== null) {
              v.return = u.return, S = v;
              break e;
            }
            S = u.return;
          }
        }
        if (R = l, ht(), $e && typeof $e.onPostCommitFiberRoot == "function") try {
          $e.onPostCommitFiberRoot(ll, e);
        } catch {
        }
        r = !0;
      }
      return r;
    } finally {
      j = n, _e.transition = t;
    }
  }
  return !1;
}
function Hu(e, t, n) {
  t = nn(n, t), t = Da(e, t, 1), e = ut(e, t, 1), t = ue(), e !== null && (Zn(e, 1, t), me(e, t));
}
function B(e, t, n) {
  if (e.tag === 3) Hu(e, e, n);
  else for (; t !== null; ) {
    if (t.tag === 3) {
      Hu(t, e, n);
      break;
    } else if (t.tag === 1) {
      var r = t.stateNode;
      if (typeof t.type.getDerivedStateFromError == "function" || typeof r.componentDidCatch == "function" && (st === null || !st.has(r))) {
        e = nn(n, e), e = Ia(t, e, 1), t = ut(t, e, 1), e = ue(), t !== null && (Zn(t, 1, e), me(t, e));
        break;
      }
    }
    t = t.return;
  }
}
function Ld(e, t, n) {
  var r = e.pingCache;
  r !== null && r.delete(t), t = ue(), e.pingedLanes |= e.suspendedLanes & n, q === e && (ee & n) === n && (X === 4 || X === 3 && (ee & 130023424) === ee && 500 > Q() - zo ? Et(e, 0) : No |= n), me(e, t);
}
function nc(e, t) {
  t === 0 && (e.mode & 1 ? (t = sr, sr <<= 1, !(sr & 130023424) && (sr = 4194304)) : t = 1);
  var n = ue();
  e = Ke(e, t), e !== null && (Zn(e, t, n), me(e, n));
}
function Md(e) {
  var t = e.memoizedState, n = 0;
  t !== null && (n = t.retryLane), nc(e, n);
}
function Rd(e, t) {
  var n = 0;
  switch (e.tag) {
    case 13:
      var r = e.stateNode, l = e.memoizedState;
      l !== null && (n = l.retryLane);
      break;
    case 19:
      r = e.stateNode;
      break;
    default:
      throw Error(y(314));
  }
  r !== null && r.delete(t), nc(e, n);
}
var rc;
rc = function(e, t, n) {
  if (e !== null) if (e.memoizedProps !== t.pendingProps || de.current) fe = !0;
  else {
    if (!(e.lanes & n) && !(t.flags & 128)) return fe = !1, wd(e, t, n);
    fe = !!(e.flags & 131072);
  }
  else fe = !1, $ && t.flags & 1048576 && ua(t, Qr, t.index);
  switch (t.lanes = 0, t.tag) {
    case 2:
      var r = t.type;
      Tr(e, t), e = t.pendingProps;
      var l = qt(t, ie.current);
      Gt(t, n), l = ko(null, t, r, e, l, n);
      var i = So();
      return t.flags |= 1, typeof l == "object" && l !== null && typeof l.render == "function" && l.$$typeof === void 0 ? (t.tag = 1, t.memoizedState = null, t.updateQueue = null, pe(r) ? (i = !0, Hr(t)) : i = !1, t.memoizedState = l.state !== null && l.state !== void 0 ? l.state : null, ho(t), l.updater = fl, t.stateNode = l, l._reactInternals = t, Ni(t, r, e, n), t = Ti(null, t, r, !0, i, n)) : (t.tag = 0, $ && i && uo(t), oe(null, t, l, n), t = t.child), t;
    case 16:
      r = t.elementType;
      e: {
        switch (Tr(e, t), e = t.pendingProps, l = r._init, r = l(r._payload), t.type = r, l = t.tag = Od(r), e = Te(r, e), l) {
          case 0:
            t = Pi(null, t, r, e, n);
            break e;
          case 1:
            t = ju(null, t, r, e, n);
            break e;
          case 11:
            t = Mu(null, t, r, e, n);
            break e;
          case 14:
            t = Ru(null, t, r, Te(r.type, e), n);
            break e;
        }
        throw Error(y(
          306,
          r,
          ""
        ));
      }
      return t;
    case 0:
      return r = t.type, l = t.pendingProps, l = t.elementType === r ? l : Te(r, l), Pi(e, t, r, l, n);
    case 1:
      return r = t.type, l = t.pendingProps, l = t.elementType === r ? l : Te(r, l), ju(e, t, r, l, n);
    case 3:
      e: {
        if (Aa(t), e === null) throw Error(y(387));
        r = t.pendingProps, i = t.memoizedState, l = i.element, pa(e, t), Xr(t, r, null, n);
        var o = t.memoizedState;
        if (r = o.element, i.isDehydrated) if (i = { element: r, isDehydrated: !1, cache: o.cache, pendingSuspenseBoundaries: o.pendingSuspenseBoundaries, transitions: o.transitions }, t.updateQueue.baseState = i, t.memoizedState = i, t.flags & 256) {
          l = nn(Error(y(423)), t), t = Ou(e, t, r, n, l);
          break e;
        } else if (r !== l) {
          l = nn(Error(y(424)), t), t = Ou(e, t, r, n, l);
          break e;
        } else for (ve = ot(t.stateNode.containerInfo.firstChild), ye = t, $ = !0, Me = null, n = fa(t, null, r, n), t.child = n; n; ) n.flags = n.flags & -3 | 4096, n = n.sibling;
        else {
          if (bt(), r === l) {
            t = Xe(e, t, n);
            break e;
          }
          oe(e, t, r, n);
        }
        t = t.child;
      }
      return t;
    case 5:
      return ma(t), e === null && Ei(t), r = t.type, l = t.pendingProps, i = e !== null ? e.memoizedProps : null, o = l.children, gi(r, l) ? o = null : i !== null && gi(r, i) && (t.flags |= 32), Ua(e, t), oe(e, t, o, n), t.child;
    case 6:
      return e === null && Ei(t), null;
    case 13:
      return Va(e, t, n);
    case 4:
      return vo(t, t.stateNode.containerInfo), r = t.pendingProps, e === null ? t.child = en(t, null, r, n) : oe(e, t, r, n), t.child;
    case 11:
      return r = t.type, l = t.pendingProps, l = t.elementType === r ? l : Te(r, l), Mu(e, t, r, l, n);
    case 7:
      return oe(e, t, t.pendingProps, n), t.child;
    case 8:
      return oe(e, t, t.pendingProps.children, n), t.child;
    case 12:
      return oe(e, t, t.pendingProps.children, n), t.child;
    case 10:
      e: {
        if (r = t.type._context, l = t.pendingProps, i = t.memoizedProps, o = l.value, D(Yr, r._currentValue), r._currentValue = o, i !== null) if (Oe(i.value, o)) {
          if (i.children === l.children && !de.current) {
            t = Xe(e, t, n);
            break e;
          }
        } else for (i = t.child, i !== null && (i.return = t); i !== null; ) {
          var u = i.dependencies;
          if (u !== null) {
            o = i.child;
            for (var s = u.firstContext; s !== null; ) {
              if (s.context === r) {
                if (i.tag === 1) {
                  s = We(-1, n & -n), s.tag = 2;
                  var c = i.updateQueue;
                  if (c !== null) {
                    c = c.shared;
                    var h = c.pending;
                    h === null ? s.next = s : (s.next = h.next, h.next = s), c.pending = s;
                  }
                }
                i.lanes |= n, s = i.alternate, s !== null && (s.lanes |= n), _i(
                  i.return,
                  n,
                  t
                ), u.lanes |= n;
                break;
              }
              s = s.next;
            }
          } else if (i.tag === 10) o = i.type === t.type ? null : i.child;
          else if (i.tag === 18) {
            if (o = i.return, o === null) throw Error(y(341));
            o.lanes |= n, u = o.alternate, u !== null && (u.lanes |= n), _i(o, n, t), o = i.sibling;
          } else o = i.child;
          if (o !== null) o.return = i;
          else for (o = i; o !== null; ) {
            if (o === t) {
              o = null;
              break;
            }
            if (i = o.sibling, i !== null) {
              i.return = o.return, o = i;
              break;
            }
            o = o.return;
          }
          i = o;
        }
        oe(e, t, l.children, n), t = t.child;
      }
      return t;
    case 9:
      return l = t.type, r = t.pendingProps.children, Gt(t, n), l = Ce(l), r = r(l), t.flags |= 1, oe(e, t, r, n), t.child;
    case 14:
      return r = t.type, l = Te(r, t.pendingProps), l = Te(r.type, l), Ru(e, t, r, l, n);
    case 15:
      return Fa(e, t, t.type, t.pendingProps, n);
    case 17:
      return r = t.type, l = t.pendingProps, l = t.elementType === r ? l : Te(r, l), Tr(e, t), t.tag = 1, pe(r) ? (e = !0, Hr(t)) : e = !1, Gt(t, n), Oa(t, r, l), Ni(t, r, l, n), Ti(null, t, r, !0, e, n);
    case 19:
      return Ba(e, t, n);
    case 22:
      return $a(e, t, n);
  }
  throw Error(y(156, t.tag));
};
function lc(e, t) {
  return Ms(e, t);
}
function jd(e, t, n, r) {
  this.tag = e, this.key = n, this.sibling = this.child = this.return = this.stateNode = this.type = this.elementType = null, this.index = 0, this.ref = null, this.pendingProps = t, this.dependencies = this.memoizedState = this.updateQueue = this.memoizedProps = null, this.mode = r, this.subtreeFlags = this.flags = 0, this.deletions = null, this.childLanes = this.lanes = 0, this.alternate = null;
}
function Ee(e, t, n, r) {
  return new jd(e, t, n, r);
}
function Mo(e) {
  return e = e.prototype, !(!e || !e.isReactComponent);
}
function Od(e) {
  if (typeof e == "function") return Mo(e) ? 1 : 0;
  if (e != null) {
    if (e = e.$$typeof, e === Gi) return 11;
    if (e === Zi) return 14;
  }
  return 2;
}
function ct(e, t) {
  var n = e.alternate;
  return n === null ? (n = Ee(e.tag, t, e.key, e.mode), n.elementType = e.elementType, n.type = e.type, n.stateNode = e.stateNode, n.alternate = e, e.alternate = n) : (n.pendingProps = t, n.type = e.type, n.flags = 0, n.subtreeFlags = 0, n.deletions = null), n.flags = e.flags & 14680064, n.childLanes = e.childLanes, n.lanes = e.lanes, n.child = e.child, n.memoizedProps = e.memoizedProps, n.memoizedState = e.memoizedState, n.updateQueue = e.updateQueue, t = e.dependencies, n.dependencies = t === null ? null : { lanes: t.lanes, firstContext: t.firstContext }, n.sibling = e.sibling, n.index = e.index, n.ref = e.ref, n;
}
function Rr(e, t, n, r, l, i) {
  var o = 2;
  if (r = e, typeof e == "function") Mo(e) && (o = 1);
  else if (typeof e == "string") o = 5;
  else e: switch (e) {
    case Ot:
      return _t(n.children, l, i, t);
    case Xi:
      o = 8, l |= 8;
      break;
    case Zl:
      return e = Ee(12, n, t, l | 2), e.elementType = Zl, e.lanes = i, e;
    case Jl:
      return e = Ee(13, n, t, l), e.elementType = Jl, e.lanes = i, e;
    case ql:
      return e = Ee(19, n, t, l), e.elementType = ql, e.lanes = i, e;
    case ms:
      return ml(n, l, i, t);
    default:
      if (typeof e == "object" && e !== null) switch (e.$$typeof) {
        case ds:
          o = 10;
          break e;
        case ps:
          o = 9;
          break e;
        case Gi:
          o = 11;
          break e;
        case Zi:
          o = 14;
          break e;
        case Je:
          o = 16, r = null;
          break e;
      }
      throw Error(y(130, e == null ? e : typeof e, ""));
  }
  return t = Ee(o, n, t, l), t.elementType = e, t.type = r, t.lanes = i, t;
}
function _t(e, t, n, r) {
  return e = Ee(7, e, r, t), e.lanes = n, e;
}
function ml(e, t, n, r) {
  return e = Ee(22, e, r, t), e.elementType = ms, e.lanes = n, e.stateNode = { isHidden: !1 }, e;
}
function Yl(e, t, n) {
  return e = Ee(6, e, null, t), e.lanes = n, e;
}
function Kl(e, t, n) {
  return t = Ee(4, e.children !== null ? e.children : [], e.key, t), t.lanes = n, t.stateNode = { containerInfo: e.containerInfo, pendingChildren: null, implementation: e.implementation }, t;
}
function Dd(e, t, n, r, l) {
  this.tag = t, this.containerInfo = e, this.finishedWork = this.pingCache = this.current = this.pendingChildren = null, this.timeoutHandle = -1, this.callbackNode = this.pendingContext = this.context = null, this.callbackPriority = 0, this.eventTimes = zl(0), this.expirationTimes = zl(-1), this.entangledLanes = this.finishedLanes = this.mutableReadLanes = this.expiredLanes = this.pingedLanes = this.suspendedLanes = this.pendingLanes = 0, this.entanglements = zl(0), this.identifierPrefix = r, this.onRecoverableError = l, this.mutableSourceEagerHydrationData = null;
}
function Ro(e, t, n, r, l, i, o, u, s) {
  return e = new Dd(e, t, n, u, s), t === 1 ? (t = 1, i === !0 && (t |= 8)) : t = 0, i = Ee(3, null, null, t), e.current = i, i.stateNode = e, i.memoizedState = { element: r, isDehydrated: n, cache: null, transitions: null, pendingSuspenseBoundaries: null }, ho(i), e;
}
function Id(e, t, n) {
  var r = 3 < arguments.length && arguments[3] !== void 0 ? arguments[3] : null;
  return { $$typeof: jt, key: r == null ? null : "" + r, children: e, containerInfo: t, implementation: n };
}
function ic(e) {
  if (!e) return dt;
  e = e._reactInternals;
  e: {
    if (Mt(e) !== e || e.tag !== 1) throw Error(y(170));
    var t = e;
    do {
      switch (t.tag) {
        case 3:
          t = t.stateNode.context;
          break e;
        case 1:
          if (pe(t.type)) {
            t = t.stateNode.__reactInternalMemoizedMergedChildContext;
            break e;
          }
      }
      t = t.return;
    } while (t !== null);
    throw Error(y(171));
  }
  if (e.tag === 1) {
    var n = e.type;
    if (pe(n)) return ia(e, n, t);
  }
  return t;
}
function oc(e, t, n, r, l, i, o, u, s) {
  return e = Ro(n, r, !0, e, l, i, o, u, s), e.context = ic(null), n = e.current, r = ue(), l = at(n), i = We(r, l), i.callback = t ?? null, ut(n, i, l), e.current.lanes = l, Zn(e, l, r), me(e, r), e;
}
function hl(e, t, n, r) {
  var l = t.current, i = ue(), o = at(l);
  return n = ic(n), t.context === null ? t.context = n : t.pendingContext = n, t = We(i, o), t.payload = { element: e }, r = r === void 0 ? null : r, r !== null && (t.callback = r), e = ut(l, t, o), e !== null && (je(e, l, o, i), Nr(e, l, o)), o;
}
function nl(e) {
  if (e = e.current, !e.child) return null;
  switch (e.child.tag) {
    case 5:
      return e.child.stateNode;
    default:
      return e.child.stateNode;
  }
}
function Wu(e, t) {
  if (e = e.memoizedState, e !== null && e.dehydrated !== null) {
    var n = e.retryLane;
    e.retryLane = n !== 0 && n < t ? n : t;
  }
}
function jo(e, t) {
  Wu(e, t), (e = e.alternate) && Wu(e, t);
}
function Fd() {
  return null;
}
var uc = typeof reportError == "function" ? reportError : function(e) {
  console.error(e);
};
function Oo(e) {
  this._internalRoot = e;
}
vl.prototype.render = Oo.prototype.render = function(e) {
  var t = this._internalRoot;
  if (t === null) throw Error(y(409));
  hl(e, t, null, null);
};
vl.prototype.unmount = Oo.prototype.unmount = function() {
  var e = this._internalRoot;
  if (e !== null) {
    this._internalRoot = null;
    var t = e.containerInfo;
    Tt(function() {
      hl(null, e, null, null);
    }), t[Ye] = null;
  }
};
function vl(e) {
  this._internalRoot = e;
}
vl.prototype.unstable_scheduleHydration = function(e) {
  if (e) {
    var t = $s();
    e = { blockedOn: null, target: e, priority: t };
    for (var n = 0; n < be.length && t !== 0 && t < be[n].priority; n++) ;
    be.splice(n, 0, e), n === 0 && As(e);
  }
};
function Do(e) {
  return !(!e || e.nodeType !== 1 && e.nodeType !== 9 && e.nodeType !== 11);
}
function yl(e) {
  return !(!e || e.nodeType !== 1 && e.nodeType !== 9 && e.nodeType !== 11 && (e.nodeType !== 8 || e.nodeValue !== " react-mount-point-unstable "));
}
function Qu() {
}
function $d(e, t, n, r, l) {
  if (l) {
    if (typeof r == "function") {
      var i = r;
      r = function() {
        var c = nl(o);
        i.call(c);
      };
    }
    var o = oc(t, r, e, 0, null, !1, !1, "", Qu);
    return e._reactRootContainer = o, e[Ye] = o.current, An(e.nodeType === 8 ? e.parentNode : e), Tt(), o;
  }
  for (; l = e.lastChild; ) e.removeChild(l);
  if (typeof r == "function") {
    var u = r;
    r = function() {
      var c = nl(s);
      u.call(c);
    };
  }
  var s = Ro(e, 0, !1, null, null, !1, !1, "", Qu);
  return e._reactRootContainer = s, e[Ye] = s.current, An(e.nodeType === 8 ? e.parentNode : e), Tt(function() {
    hl(t, s, n, r);
  }), s;
}
function gl(e, t, n, r, l) {
  var i = n._reactRootContainer;
  if (i) {
    var o = i;
    if (typeof l == "function") {
      var u = l;
      l = function() {
        var s = nl(o);
        u.call(s);
      };
    }
    hl(t, o, e, l);
  } else o = $d(n, t, e, l, r);
  return nl(o);
}
Is = function(e) {
  switch (e.tag) {
    case 3:
      var t = e.stateNode;
      if (t.current.memoizedState.isDehydrated) {
        var n = kn(t.pendingLanes);
        n !== 0 && (bi(t, n | 1), me(t, Q()), !(R & 6) && (rn = Q() + 500, ht()));
      }
      break;
    case 13:
      Tt(function() {
        var r = Ke(e, 1);
        if (r !== null) {
          var l = ue();
          je(r, e, 1, l);
        }
      }), jo(e, 1);
  }
};
eo = function(e) {
  if (e.tag === 13) {
    var t = Ke(e, 134217728);
    if (t !== null) {
      var n = ue();
      je(t, e, 134217728, n);
    }
    jo(e, 134217728);
  }
};
Fs = function(e) {
  if (e.tag === 13) {
    var t = at(e), n = Ke(e, t);
    if (n !== null) {
      var r = ue();
      je(n, e, t, r);
    }
    jo(e, t);
  }
};
$s = function() {
  return j;
};
Us = function(e, t) {
  var n = j;
  try {
    return j = e, t();
  } finally {
    j = n;
  }
};
si = function(e, t, n) {
  switch (t) {
    case "input":
      if (ti(e, n), t = n.name, n.type === "radio" && t != null) {
        for (n = e; n.parentNode; ) n = n.parentNode;
        for (n = n.querySelectorAll("input[name=" + JSON.stringify("" + t) + '][type="radio"]'), t = 0; t < n.length; t++) {
          var r = n[t];
          if (r !== e && r.form === e.form) {
            var l = sl(r);
            if (!l) throw Error(y(90));
            vs(r), ti(r, l);
          }
        }
      }
      break;
    case "textarea":
      gs(e, n);
      break;
    case "select":
      t = n.value, t != null && Qt(e, !!n.multiple, t, !1);
  }
};
Cs = Po;
Ns = Tt;
var Ud = { usingClientEntryPoint: !1, Events: [qn, $t, sl, Es, _s, Po] }, yn = { findFiberByHostInstance: kt, bundleType: 0, version: "18.3.1", rendererPackageName: "react-dom" }, Ad = { bundleType: yn.bundleType, version: yn.version, rendererPackageName: yn.rendererPackageName, rendererConfig: yn.rendererConfig, overrideHookState: null, overrideHookStateDeletePath: null, overrideHookStateRenamePath: null, overrideProps: null, overridePropsDeletePath: null, overridePropsRenamePath: null, setErrorHandler: null, setSuspenseHandler: null, scheduleUpdate: null, currentDispatcherRef: Ge.ReactCurrentDispatcher, findHostInstanceByFiber: function(e) {
  return e = Ts(e), e === null ? null : e.stateNode;
}, findFiberByHostInstance: yn.findFiberByHostInstance || Fd, findHostInstancesForRefresh: null, scheduleRefresh: null, scheduleRoot: null, setRefreshHandler: null, getCurrentFiber: null, reconcilerVersion: "18.3.1-next-f1338f8080-20240426" };
if (typeof __REACT_DEVTOOLS_GLOBAL_HOOK__ < "u") {
  var wr = __REACT_DEVTOOLS_GLOBAL_HOOK__;
  if (!wr.isDisabled && wr.supportsFiber) try {
    ll = wr.inject(Ad), $e = wr;
  } catch {
  }
}
we.__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED = Ud;
we.createPortal = function(e, t) {
  var n = 2 < arguments.length && arguments[2] !== void 0 ? arguments[2] : null;
  if (!Do(t)) throw Error(y(200));
  return Id(e, t, null, n);
};
we.createRoot = function(e, t) {
  if (!Do(e)) throw Error(y(299));
  var n = !1, r = "", l = uc;
  return t != null && (t.unstable_strictMode === !0 && (n = !0), t.identifierPrefix !== void 0 && (r = t.identifierPrefix), t.onRecoverableError !== void 0 && (l = t.onRecoverableError)), t = Ro(e, 1, !1, null, null, n, !1, r, l), e[Ye] = t.current, An(e.nodeType === 8 ? e.parentNode : e), new Oo(t);
};
we.findDOMNode = function(e) {
  if (e == null) return null;
  if (e.nodeType === 1) return e;
  var t = e._reactInternals;
  if (t === void 0)
    throw typeof e.render == "function" ? Error(y(188)) : (e = Object.keys(e).join(","), Error(y(268, e)));
  return e = Ts(t), e = e === null ? null : e.stateNode, e;
};
we.flushSync = function(e) {
  return Tt(e);
};
we.hydrate = function(e, t, n) {
  if (!yl(t)) throw Error(y(200));
  return gl(null, e, t, !0, n);
};
we.hydrateRoot = function(e, t, n) {
  if (!Do(e)) throw Error(y(405));
  var r = n != null && n.hydratedSources || null, l = !1, i = "", o = uc;
  if (n != null && (n.unstable_strictMode === !0 && (l = !0), n.identifierPrefix !== void 0 && (i = n.identifierPrefix), n.onRecoverableError !== void 0 && (o = n.onRecoverableError)), t = oc(t, null, e, 1, n ?? null, l, !1, i, o), e[Ye] = t.current, An(e), r) for (e = 0; e < r.length; e++) n = r[e], l = n._getVersion, l = l(n._source), t.mutableSourceEagerHydrationData == null ? t.mutableSourceEagerHydrationData = [n, l] : t.mutableSourceEagerHydrationData.push(
    n,
    l
  );
  return new vl(t);
};
we.render = function(e, t, n) {
  if (!yl(t)) throw Error(y(200));
  return gl(null, e, t, !1, n);
};
we.unmountComponentAtNode = function(e) {
  if (!yl(e)) throw Error(y(40));
  return e._reactRootContainer ? (Tt(function() {
    gl(null, null, e, !1, function() {
      e._reactRootContainer = null, e[Ye] = null;
    });
  }), !0) : !1;
};
we.unstable_batchedUpdates = Po;
we.unstable_renderSubtreeIntoContainer = function(e, t, n, r) {
  if (!yl(n)) throw Error(y(200));
  if (e == null || e._reactInternals === void 0) throw Error(y(38));
  return gl(e, t, n, !1, r);
};
we.version = "18.3.1-next-f1338f8080-20240426";
function sc() {
  if (!(typeof __REACT_DEVTOOLS_GLOBAL_HOOK__ > "u" || typeof __REACT_DEVTOOLS_GLOBAL_HOOK__.checkDCE != "function"))
    try {
      __REACT_DEVTOOLS_GLOBAL_HOOK__.checkDCE(sc);
    } catch (e) {
      console.error(e);
    }
}
sc(), ss.exports = we;
var Vd = ss.exports, ac, Yu = Vd;
ac = Yu.createRoot, Yu.hydrateRoot;
async function Bd(e, t = {}, n) {
  return window.__TAURI_INTERNALS__.invoke(e, t, n);
}
const Hd = "(prefers-reduced-motion: reduce)";
function Wd() {
  const e = () => typeof window.matchMedia == "function" ? window.matchMedia(Hd) : null, [t, n] = J.useState(() => {
    var r;
    return ((r = e()) == null ? void 0 : r.matches) ?? !1;
  });
  return J.useEffect(() => {
    const r = e();
    if (!r || typeof r.addEventListener != "function") return;
    const l = () => n(r.matches);
    return r.addEventListener("change", l), () => r.removeEventListener("change", l);
  }, []), t;
}
function Qd({ tone: e, small: t = !1, cue: n = null, quotaPct: r = null, busy: l = !1, errors: i = 0 }) {
  const o = Yd({ tone: e, cue: n, quotaPct: r, busy: l, errors: i }), u = Wd(), s = np(!u && o === "open", e === "running"), c = o === "open" && (n === "wink" || s === "wink"), h = o === "open" && s === "curious", m = o === "open" || o === "wide" || o === "worried" || o === "focus" || o === "drowsy", p = s === "glance" ? Zd : s === "ponder" ? Jd : null, g = bd(!u && m && !p), w = p ?? g, k = ep(!u && (o === "open" || o === "worried" || o === "focus" || o === "drowsy")), O = J.useId().replace(/[^a-zA-Z0-9]/g, ""), d = qd[o], a = !t && e === "running" && (o === "open" || o === "focus");
  return /* @__PURE__ */ z.jsxs("span", { className: `isl-eyes${t ? " is-small" : ""}`, "data-tone": e, "data-mode": o, "aria-hidden": "true", children: [
    a && /* @__PURE__ */ z.jsx("span", { className: "isl-spark is-l", children: "+" }),
    !t && (o === "wide" || o === "plead") && /* @__PURE__ */ z.jsx("span", { className: "isl-spark is-l is-warn", children: "!" }),
    /* @__PURE__ */ z.jsx("svg", { className: "isl-eye-pair", width: t ? 20 : 26, height: t ? 13 : 16, viewBox: "0 0 26 16", children: o === "dead" ? /* @__PURE__ */ z.jsxs("g", { fill: "none", stroke: "currentColor", strokeWidth: 2.4, strokeLinecap: "round", children: [
      /* @__PURE__ */ z.jsx("path", { d: "M3 4.5l7 7M10 4.5l-7 7" }),
      /* @__PURE__ */ z.jsx("path", { d: "M16 4.5l7 7M23 4.5l-7 7" })
    ] }) : o === "frustrated" || o === "yawn" ? /* @__PURE__ */ z.jsxs("g", { className: "isl-eye-squeeze", fill: "none", stroke: "currentColor", strokeWidth: 2.4, strokeLinecap: "round", strokeLinejoin: "round", children: [
      /* @__PURE__ */ z.jsx("path", { d: "M3.5 4.5 L9.5 8 L3.5 11.5" }),
      /* @__PURE__ */ z.jsx("path", { d: "M22.5 4.5 L16.5 8 L22.5 11.5" })
    ] }) : o === "plead" ? /* @__PURE__ */ z.jsx("g", { className: "isl-eye-plead", children: [6.5, 19.5].map((f) => /* @__PURE__ */ z.jsxs("g", { children: [
      /* @__PURE__ */ z.jsx("ellipse", { className: "isl-sclera", cx: f, cy: 8, rx: 5.8, ry: 7 }),
      /* @__PURE__ */ z.jsx("circle", { className: "isl-pupil", cx: f, cy: 7, r: 3.7 }),
      /* @__PURE__ */ z.jsx("circle", { className: "isl-glint", cx: f + 1.3, cy: 5.4, r: 1.25 }),
      /* @__PURE__ */ z.jsx("circle", { className: "isl-glint", cx: f - 1.2, cy: 8.6, r: 0.6 })
    ] }, f)) }) : o === "happy" || o === "blush" ? /* @__PURE__ */ z.jsxs("g", { fill: "none", stroke: "currentColor", strokeWidth: 2.6, strokeLinecap: "round", children: [
      /* @__PURE__ */ z.jsx("path", { d: Xl[0] }),
      /* @__PURE__ */ z.jsx("path", { d: Xl[1] }),
      o === "blush" && /* @__PURE__ */ z.jsxs("g", { className: "isl-cheeks", stroke: "none", children: [
        /* @__PURE__ */ z.jsx("ellipse", { cx: 4, cy: 13.6, rx: 2.8, ry: 1.4 }),
        /* @__PURE__ */ z.jsx("ellipse", { cx: 22, cy: 13.6, rx: 2.8, ry: 1.4 })
      ] })
    ] }) : o === "sleep" ? /* @__PURE__ */ z.jsxs("g", { fill: "none", stroke: "currentColor", strokeWidth: 2.4, strokeLinecap: "round", children: [
      /* @__PURE__ */ z.jsx("path", { d: "M2.5 9 Q6.5 11.5 10.5 9" }),
      /* @__PURE__ */ z.jsx("path", { d: "M15.5 9 Q19.5 11.5 23.5 9" })
    ] }) : o === "star" ? /* @__PURE__ */ z.jsxs("g", { className: "isl-eye-pop", fill: "currentColor", children: [
      /* @__PURE__ */ z.jsx("path", { d: Ku(6.5, 8) }),
      /* @__PURE__ */ z.jsx("path", { d: Ku(19.5, 8) })
    ] }) : o === "dizzy" ? /* @__PURE__ */ z.jsxs("g", { fill: "none", stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round", children: [
      /* @__PURE__ */ z.jsx("path", { className: "isl-spiral", d: Xu(6.5, 8) }),
      /* @__PURE__ */ z.jsx("path", { className: "isl-spiral", d: Xu(19.5, 8) })
    ] }) : o === "surprised" ? /* @__PURE__ */ z.jsx("g", { className: "isl-eye-pop", children: [6.5, 19.5].map((f) => /* @__PURE__ */ z.jsxs("g", { children: [
      /* @__PURE__ */ z.jsx("circle", { className: "isl-sclera", cx: f, cy: 8, r: 6.4 }),
      /* @__PURE__ */ z.jsx("circle", { className: "isl-pupil", cx: f, cy: 8, r: 1.4 })
    ] }, f)) }) : /* @__PURE__ */ z.jsxs("g", { className: `isl-eye-open${k ? " is-blink" : ""}`, children: [
      d && /* @__PURE__ */ z.jsx("defs", { children: d.map((f, v) => /* @__PURE__ */ z.jsx("clipPath", { id: `${O}-${v}`, children: /* @__PURE__ */ z.jsx("polygon", { points: f }) }, v)) }),
      [6.5, 19.5].map((f, v) => c && v === 1 ? /* @__PURE__ */ z.jsx("path", { d: Xl[1], fill: "none", stroke: "currentColor", strokeWidth: 2.6, strokeLinecap: "round" }, f) : /* @__PURE__ */ z.jsxs("g", { clipPath: d ? `url(#${O}-${v})` : void 0, children: [
        /* @__PURE__ */ z.jsx("ellipse", { className: "isl-sclera", cx: f, cy: 8, rx: o === "wide" ? 5.6 : h && v === 0 ? 6 : 5, ry: o === "wide" ? 7.2 : h && v === 0 ? 7.6 : 6.6 }),
        /* @__PURE__ */ z.jsx(
          "circle",
          {
            className: "isl-pupil",
            cx: f,
            cy: 8,
            r: o === "wide" ? 1.9 : h && v === 0 ? 2 : 2.5,
            style: { transform: `translate(${w.x * 2.2}px, ${w.y * 2.6}px)` }
          }
        )
      ] }, f))
    ] }) }),
    a && /* @__PURE__ */ z.jsx("span", { className: "isl-spark is-r", children: "+" }),
    !t && o === "star" && /* @__PURE__ */ z.jsx("span", { className: "isl-spark is-r is-star", children: "✦" }),
    !t && o === "worried" && /* @__PURE__ */ z.jsx("span", { className: "isl-sweat" }),
    !t && o === "sleep" && /* @__PURE__ */ z.jsx("span", { className: "isl-zz", children: "z" }),
    !t && o === "yawn" && /* @__PURE__ */ z.jsx("span", { className: "isl-tear" }),
    !t && o === "frustrated" && /* @__PURE__ */ z.jsx("span", { className: "isl-spark is-r is-err", children: "#" })
  ] });
}
function Yd({ tone: e, cue: t = null, quotaPct: n = null, busy: r = !1, errors: l = 0 }) {
  const i = ip(e === "idle"), o = Gu(i !== "asleep", 900), u = Gu(i === "asleep", Xd), s = op(e === "pending", Kd), c = n != null && n >= 100, h = n != null && n >= Gd;
  return e === "error" ? l >= 2 ? "frustrated" : "dead" : e === "pending" ? s ? "plead" : "wide" : c ? "dizzy" : t === "star" || t === "surprised" || t === "blush" ? t : o ? "surprised" : u ? "yawn" : e === "waiting" ? "happy" : h ? "worried" : r && e === "running" ? "focus" : i === "asleep" ? "sleep" : i === "drowsy" ? "drowsy" : "open";
}
const Kd = 6e4, Xd = 1800, Gd = 85, Xl = ["M2.5 10.5 Q6.5 3.5 10.5 10.5", "M15.5 10.5 Q19.5 3.5 23.5 10.5"], Zd = { x: -1, y: 0.2 }, Jd = { x: 0.7, y: -1 }, qd = {
  worried: ["-1,6.5 13,0.6 13,18 -1,18", "13,0.6 27,6.5 27,18 13,18"],
  focus: ["-1,1.6 13,5.2 13,18 -1,18", "13,5.2 27,1.6 27,18 13,18"],
  drowsy: ["-1,7.4 13,7.4 13,18 -1,18", "13,7.4 27,7.4 27,18 13,18"]
};
function Ku(e, t) {
  const n = [];
  for (let r = 0; r < 10; r += 1) {
    const l = r % 2 === 0 ? 6.2 : 2.6, i = -Math.PI / 2 + r * Math.PI / 5;
    n.push(`${(e + l * Math.cos(i)).toFixed(2)} ${(t + l * Math.sin(i)).toFixed(2)}`);
  }
  return `M${n.join("L")}Z`;
}
function Xu(e, t) {
  const n = [];
  for (let r = 0; r <= 40; r += 1) {
    const l = r / 40 * Math.PI * 5, i = 0.4 + r / 40 * 5.4;
    n.push(`${(e + i * Math.cos(l)).toFixed(2)} ${(t + i * Math.sin(l)).toFixed(2)}`);
  }
  return `M${n.join("L")}`;
}
function bd(e) {
  const [t, n] = J.useState({ x: 0, y: 0 });
  return J.useEffect(() => {
    if (!e) {
      n({ x: 0, y: 0 });
      return;
    }
    let r = !1, l = 0, i = null, o = 0;
    const u = () => {
      r || (l = window.setTimeout(s, o >= 5 ? 600 : 110));
    }, s = () => {
      if (document.visibilityState !== "visible") {
        u();
        return;
      }
      Bd("island_cursor").then((c) => {
        if (r || !c) return;
        const [h, m] = c;
        o = i && Math.abs(i[0] - h) < 1 && Math.abs(i[1] - m) < 1 ? o + 1 : 0, i = [h, m];
        const p = Math.max(-1, Math.min(1, h / 150)), g = Math.max(-1, Math.min(1, m / 60));
        n((w) => Math.abs(w.x - p) < 0.05 && Math.abs(w.y - g) < 0.05 ? w : { x: p, y: g });
      }).catch(() => {
      }).finally(u);
    };
    return s(), () => {
      r = !0, window.clearTimeout(l);
    };
  }, [e]), t;
}
function ep(e) {
  const [t, n] = J.useState(!1);
  return J.useEffect(() => {
    if (!e) return;
    let r = 0;
    const l = () => {
      r = window.setTimeout(() => {
        const i = Math.random() < 0.2;
        n(!0), r = window.setTimeout(() => {
          n(!1), i ? r = window.setTimeout(() => {
            n(!0), r = window.setTimeout(() => {
              n(!1), l();
            }, 130);
          }, 160) : l();
        }, 130);
      }, 2500 + Math.random() * 3500);
    };
    return l(), () => {
      window.clearTimeout(r), n(!1);
    };
  }, [e]), t;
}
const tp = { wink: 500, glance: 900, curious: 1400, ponder: 1600 };
function np(e, t) {
  const [n, r] = J.useState(null);
  return J.useEffect(() => {
    if (!e) return;
    let l = 0;
    const i = () => {
      l = window.setTimeout(() => {
        if (document.visibilityState !== "visible") {
          i();
          return;
        }
        const o = t ? ["wink", "glance", "curious", "ponder", "ponder"] : ["wink", "glance", "curious"], u = o[Math.floor(Math.random() * o.length)];
        r(u), l = window.setTimeout(() => {
          r(null), i();
        }, tp[u]);
      }, 18e3 + Math.random() * 22e3);
    };
    return i(), () => {
      window.clearTimeout(l), r(null);
    };
  }, [e, t]), n;
}
const rp = 6 * 6e4, lp = 10 * 6e4;
function ip(e) {
  const [t, n] = J.useState("awake");
  return J.useEffect(() => {
    if (!e) {
      n("awake");
      return;
    }
    n((/* @__PURE__ */ new Date()).getHours() < 5 ? "drowsy" : "awake");
    const r = window.setTimeout(() => n("drowsy"), rp), l = window.setTimeout(() => n("asleep"), lp);
    return () => {
      window.clearTimeout(r), window.clearTimeout(l);
    };
  }, [e]), t;
}
function Gu(e, t) {
  const [n, r] = J.useState(!1), l = J.useRef(e);
  return J.useEffect(() => {
    const i = l.current;
    if (l.current = e, !e) {
      r(!1);
      return;
    }
    if (i) return;
    r(!0);
    const o = window.setTimeout(() => r(!1), t);
    return () => window.clearTimeout(o);
  }, [e, t]), n;
}
function op(e, t) {
  const [n, r] = J.useState(!1);
  return J.useEffect(() => {
    if (r(!1), !e) return;
    const l = window.setTimeout(() => r(!0), t);
    return () => window.clearTimeout(l);
  }, [e, t]), n;
}
const up = `:host {
  
  --cc-ok-vivid: #30d158;
  --cc-warn-vivid: #ffb829;
  --cc-err-vivid: #ff453a;
  --isl-ink: #000;
  --isl-text: #f5f5f7;
  --isl-text-2: #b9b9be;
  --isl-dim: #98989d;
  --isl-faint: #5e5e62;
  --isl-chip: #1c1c1e;
  --isl-chip-hover: #2c2c2e;
  --isl-track: #3a3a3c;
  --isl-line: rgba(255, 255, 255, 0.1);
  
  --isl-lime: #d4f64c;
  --isl-lime-dim: #c3e63a;
  --isl-on-lime: #1c2600;
  --isl-ok: var(--cc-ok-vivid);
  
  --isl-warn: #ffb829;
  
  --isl-pending: var(--cc-warn-vivid);
  --isl-waiting: var(--cc-warn-vivid);
  --isl-err: var(--cc-err-vivid);
  --isl-ear: 14px;
}
.isl-eyes { position: relative; display: inline-flex; align-items: center; padding: 0 6px; flex: none; color: #fff; }
.isl-eyes.is-small { padding: 0; }
.isl-eye-pair { display: block; overflow: visible; }
.isl-sclera { fill: #fff; }
.isl-pupil { fill: #000; transition: transform var(--dur-base) var(--ease-out); }
.isl-eye-open { transform-box: fill-box; transform-origin: center; transition: transform calc(var(--dur-fast) / 2) var(--ease-in); }
.isl-eye-open.is-blink { transform: scaleY(0.12); }
.isl-spark.is-warn { color: var(--isl-pending); font-size: 11px; top: -5px; }
.isl-eyes[data-mode="wide"] .isl-eye-pair { animation: isl-bounce var(--dur-bounce) ease-in-out infinite; }
.isl-eyes[data-tone="error"] { color: var(--isl-err); }
@keyframes isl-bounce { 0%, 100% { transform: translateY(0); } 40% { transform: translateY(-2px); } 60% { transform: translateY(0.5px); } }
.isl-zz { position: absolute; right: -4px; top: -6px; font-size: 9px; font-weight: 600; color: var(--isl-dim); animation: isl-zz var(--dur-idle) ease-in infinite; }
@keyframes isl-zz { 0% { opacity: 0; transform: translate(0, 4px) scale(0.7); } 30% { opacity: 1; } 100% { opacity: 0; transform: translate(5px, -8px) scale(1.15); } }
.isl-spark { position: absolute; top: -3px; font-size: 9px; font-weight: 600; line-height: 1; color: var(--isl-ok); opacity: 0.85; }
.isl-spark.is-l { left: -1px; }
.isl-spark.is-r { right: -2px; top: -6px; }
.isl-eyes[data-tone="running"] .isl-spark { animation: isl-twinkle var(--dur-idle) ease-in-out infinite; }
.isl-eyes[data-mode="happy"] .isl-eye-pair { animation: isl-giggle var(--dur-idle) ease-in-out infinite; }
@keyframes isl-giggle { 0%, 70%, 100% { transform: translateY(0); } 80% { transform: translateY(-1.5px); } 90% { transform: translateY(0); } }
.isl-eyes[data-tone="running"] .isl-spark.is-r { animation-delay: calc(var(--dur-idle) / 2); }
.isl-eye-pop { transform-box: fill-box; transform-origin: center; animation: isl-pop var(--dur-slower) cubic-bezier(0.3, 1.6, 0.5, 1); }
@keyframes isl-pop { from { transform: scale(0.4); } to { transform: scale(1); } }
.isl-eyes[data-mode="star"] .isl-eye-pair { animation: isl-giggle var(--dur-pulse) ease-in-out infinite; }
.isl-spark.is-star { color: var(--isl-waiting); font-size: 9px; animation: isl-twinkle var(--dur-pulse-fast) ease-in-out infinite; }
.isl-eyes[data-mode="frustrated"] .isl-eye-pair { animation: isl-shake calc(var(--dur-pulse-fast) / 2) ease-in-out infinite; }
@keyframes isl-shake { 0%, 100% { transform: translateX(0); } 25% { transform: translateX(-1px); } 75% { transform: translateX(1px); } }
.isl-spark.is-err { color: var(--isl-err); font-size: 10px; animation: isl-twinkle var(--dur-pulse-fast) ease-in-out infinite; }
.isl-eyes[data-mode="yawn"] .isl-eye-squeeze { transform-box: fill-box; transform-origin: center; animation: isl-yawn calc(var(--dur-idle) * 0.75) ease-in-out; }
@keyframes isl-yawn { 0% { transform: scaleY(1.4); } 40%, 70% { transform: scaleY(0.8); } 100% { transform: scaleY(1); } }
.isl-tear { position: absolute; left: 3px; bottom: -1px; width: 3px; height: 4px; border-radius: 50% 50% 50% 50% / 65% 65% 35% 35%; background: #8fd3ff; animation: isl-fade-in calc(var(--dur-slower) * 1.4) ease; }
.isl-glint { fill: #fff; }
.isl-eye-plead { transform-box: fill-box; transform-origin: center bottom; animation: isl-plead var(--dur-idle) ease-in-out infinite; }
@keyframes isl-plead { 0%, 100% { transform: rotate(0); } 30% { transform: rotate(-3deg); } 60% { transform: rotate(3deg); } }
.isl-cheeks { fill: #ff7aa8; opacity: 0.75; animation: isl-fade-in calc(var(--dur-slower) * 1.4) ease; }
.isl-spiral { transform-box: fill-box; transform-origin: center; animation: isl-spin-plain var(--dur-orbit) linear infinite; }
@keyframes isl-spin-plain { to { transform: rotate(360deg); } }
.isl-sweat { position: absolute; right: 0; top: 0; z-index: var(--z-raised); width: 4px; height: 6px; border-radius: 50% 50% 50% 50% / 65% 65% 35% 35%; background: #8fd3ff; animation: isl-sweat var(--dur-idle) ease-in infinite; }
@keyframes isl-sweat { 0% { opacity: 0; transform: translateY(-2px); } 20% { opacity: 0.95; } 80% { opacity: 0.9; transform: translateY(5px); } 100% { opacity: 0; transform: translateY(7px); } }
@keyframes isl-look {
  0%, 18% { transform: translateX(0); }
  28%, 44% { transform: translateX(-2px); }
  56%, 72% { transform: translateX(2px); }
  82%, 100% { transform: translateX(0); }
}
@keyframes isl-blink {
  0%, 88% { transform: scaleY(1); }
  90% { transform: scaleY(0.1); }
  92% { transform: scaleY(1); }
  94% { transform: scaleY(0.1); }
  96%, 100% { transform: scaleY(1); }
}
@keyframes isl-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.45; } }
@keyframes isl-twinkle { 0%, 100% { opacity: 0.25; transform: scale(0.8); } 50% { opacity: 1; transform: scale(1); } }
@keyframes isl-spin { from { transform: rotate(-90deg); } to { transform: rotate(270deg); } }
@keyframes isl-sweep { from { stroke-dashoffset: var(--isl-c); } }
@keyframes isl-fade-in { from { opacity: 0; transform: scale(0.9); } to { opacity: 1; transform: scale(1); } }
@keyframes isl-grow {
  from { clip-path: inset(0 28% 34% 28% round 0 0 22px 22px); }
  to { clip-path: inset(0 0 0 0 round 0 0 24px 24px); }
}
@media (prefers-reduced-motion: reduce) {
.isl-notch.is-alert, .island-shell.is-entering, .island-shell.is-leaving,
  .isl-eye-pair, .isl-zz, .isl-spark, .isl-eye-pop, .isl-spiral, .isl-sweat, .isl-eye-plead, .isl-eye-squeeze, .isl-cheeks, .isl-tear, .isl-dot, .isl-ring.is-spin > svg, .isl-ring-arc { animation: none !important; }
.isl-agents .isl-agent, .isl-dots-nav button::before, .isl-pupil, .isl-eye-open { transition: none; }
}`, Io = 38, cc = 16, Mn = { x: -1, y: -1 };
window.addEventListener("pointermove", (e) => {
  Mn.x = e.clientX, Mn.y = e.clientY;
}, { passive: !0 });
function sp() {
  if (Mn.x < 0) return [0, 0];
  let e = [0, 0], t = 1 / 0;
  for (const n of document.querySelectorAll("meowo-eyes")) {
    const r = n.getBoundingClientRect();
    if (r.bottom < 0 || r.top > innerHeight || r.width === 0) continue;
    const l = Mn.x - (r.left + r.width / 2), i = Mn.y - (r.top + r.height / 2), o = Math.hypot(l, i);
    if (o < t) {
      t = o;
      const u = r.width / Io;
      e = [l / u, i / u];
    }
  }
  return e;
}
const Zu = window;
Zu.__TAURI_INTERNALS__ || (Zu.__TAURI_INTERNALS__ = {
  invoke: (e) => Promise.resolve(e === "island_cursor" ? sp() : null)
});
let kr = null;
function ap() {
  return kr || (kr = new CSSStyleSheet(), kr.replaceSync(`${up}
:host { display: inline-block; line-height: 0; vertical-align: middle; }
.wrap { transform-origin: top left; width: ${Io}px; height: ${cc}px; }`)), kr;
}
const cp = ["running", "pending", "waiting", "error", "idle"], fp = ["star", "surprised", "blush", "wink"];
class fc extends HTMLElement {
  constructor() {
    super(...arguments);
    nr(this, "root", null);
    nr(this, "mount", null);
  }
  connectedCallback() {
    if (!this.shadowRoot) {
      const n = this.attachShadow({ mode: "open" });
      n.adoptedStyleSheets = [ap()], this.mount = document.createElement("div"), this.mount.className = "wrap", n.appendChild(this.mount), this.root = ac(this.mount);
    }
    this.render();
  }
  disconnectedCallback() {
  }
  attributeChangedCallback() {
    this.root && this.render();
  }
  render() {
    var c;
    const n = cp.includes(this.getAttribute("tone") ?? "") ? this.getAttribute("tone") : "running", r = this.getAttribute("cue"), l = fp.includes(r ?? "") ? r : null, i = this.getAttribute("quota"), o = i == null || i === "" ? null : Number(i), u = Number(this.getAttribute("size")) || 120, s = u / Io;
    this.style.width = `${u}px`, this.style.height = `${cc * s}px`, this.mount && (this.mount.style.transform = `scale(${s})`), (c = this.root) == null || c.render(
      /* @__PURE__ */ z.jsx(
        Qd,
        {
          tone: n,
          cue: l,
          quotaPct: o,
          busy: this.hasAttribute("busy"),
          errors: Number(this.getAttribute("errors")) || 0
        }
      )
    );
  }
}
nr(fc, "observedAttributes", ["tone", "cue", "quota", "busy", "errors", "size"]);
customElements.get("meowo-eyes") || customElements.define("meowo-eyes", fc);
