/*
    SPDX-License-Identifier: MIT

    Pure helpers shared by the renderers: URL handling (mirrors
    Lively.Common.Helpers.StreamUtil), scaler names and JS literal encoding.
    Instantiated as `LivelyUtil { id: util }` where needed.
*/

import QtQml

QtObject {
    // file:// URL for an absolute local path; a value that already is a URL is returned unchanged.
    function localFileUrl(path) {
        if (path === "")
            return ""
        if (/^[a-zA-Z][a-zA-Z0-9+.\-]*:\/\//.test(path))
            return path
        return "file://" + path.split("/").map(encodeURIComponent).join("/")
    }

    function fileName(path) {
        return path.substring(path.lastIndexOf("/") + 1)
    }

    function directoryOf(path) {
        var slash = path.lastIndexOf("/")
        return slash < 0 ? "" : path.substring(0, slash)
    }

    // lp_dropdown_scaler values (0 none, 1 fill, 2 uniform, 3 uniformFill); "" for anything else.
    function scalerFromIndex(index) {
        switch (index) {
        case 0: return "none"
        case 1: return "fill"
        case 2: return "uniform"
        case 3: return "uniformFill"
        default: return ""
        }
    }

    // File name suffixes accepted for a cmd_screenshot Format (0 jpeg, 1 png, 2 webp, 3 bmp).
    function screenshotSuffixes(format) {
        switch (format) {
        case 0: return ["jpg", "jpeg"]
        case 1: return ["png"]
        case 2: return ["webp"]
        case 3: return ["bmp"]
        default: return []
        }
    }

    // JavaScript source literal for a value (JSON, plus the two line terminators JSON leaves unescaped).
    function jsLiteral(value) {
        if (value === undefined)
            return "null"
        return JSON.stringify(value).replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029")
    }

    // Minimal absolute URI parser producing .NET System.Uri-like host, segments and query.
    function parseUri(address) {
        var match = /^([a-zA-Z][a-zA-Z0-9+.\-]*):\/\/([^\/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/.exec(address)
        if (match === null)
            return null
        var authority = match[2]
        var host = authority.replace(/^.*@/, "").replace(/:\d*$/, "").toLowerCase()
        var path = match[3] === "" ? "/" : match[3]
        var parts = path.split("/")
        var segments = ["/"]
        for (var i = 1; i < parts.length; i++) {
            var segment = parts[i] + (i < parts.length - 1 ? "/" : "")
            if (segment !== "")
                segments.push(segment)
        }
        return { host: host, path: path, segments: segments, query: match[4] === undefined ? "" : match[4] }
    }

    function decodeQueryComponent(text) {
        try {
            return decodeURIComponent(text.replace(/\+/g, " "))
        } catch (error) {
            return text
        }
    }

    // HttpUtility.ParseQueryString equivalent: object of key -> value (last wins).
    function parseQuery(query) {
        var result = {}
        var text = query.indexOf("?") === 0 ? query.substring(1) : query
        if (text === "")
            return result
        var pairs = text.split("&")
        for (var i = 0; i < pairs.length; i++) {
            if (pairs[i] === "")
                continue
            var eq = pairs[i].indexOf("=")
            var key = eq < 0 ? pairs[i] : pairs[i].substring(0, eq)
            var value = eq < 0 ? "" : pairs[i].substring(eq + 1)
            result[decodeQueryComponent(key)] = decodeQueryComponent(value)
        }
        return result
    }

    // Port of StreamUtil.TryParseYouTubeVideoIdFromUrl; returns the id or null.
    function youTubeVideoId(address) {
        var uri = parseUri(address)
        if (uri === null)
            uri = parseUri("http://" + address)
        if (uri === null)
            return null
        var hosts = ["www.youtube.com", "youtube.com", "youtu.be", "www.youtu.be"]
        if (hosts.indexOf(uri.host) < 0)
            return null
        var query = parseQuery(uri.query)
        var match
        if (query.hasOwnProperty("v")) {
            match = /^[a-zA-Z0-9_\-]{11}$/.exec(query["v"])
            return match === null ? null : match[0]
        }
        if (query.hasOwnProperty("u")) {
            match = /\/watch\?v=([a-zA-Z0-9_\-]{11})/.exec(query["u"])
            return match === null ? null : match[1]
        }
        var segments = uri.segments
        var last = segments[segments.length - 1].split("/").join("")
        if (/^v=[a-zA-Z0-9_\-]{11}$/.test(last))
            return last.replace("v=", "")
        if (segments.length > 2 && segments[segments.length - 2] !== "v/" && segments[segments.length - 2] !== "watch/")
            return null
        match = /^[a-zA-Z0-9_\-]{11}$/.exec(last)
        return match === null ? null : match[0]
    }

    // Port of StreamUtil.TryParseShadertoy: the embed page URL with the same query, or null.
    function shadertoyEmbedUrl(address) {
        if (address.indexOf("shadertoy.com/view") < 0)
            return null
        return address.split("view/").join("embed/") + "?gui=false&t=10&paused=false&muted=true"
    }

    // URL loaded for Kind "url", mirroring Lively.Player.WebView2 Form1 (WebPageType.online).
    function onlineUrl(address) {
        var shader = shadertoyEmbedUrl(address)
        if (shader !== null)
            return shader
        var id = youTubeVideoId(address)
        if (id !== null)
            return "https://www.youtube.com/embed/" + id + "?version=3&rel=0&autoplay=1&loop=1&controls=0&playlist=" + id
        return address
    }
}
