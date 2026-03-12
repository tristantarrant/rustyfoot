// SPDX-FileCopyrightText: 2012-2023 MOD Audio UG
// SPDX-License-Identifier: AGPL-3.0-or-later

// add this to plugin data when cloud fails
function getDummyPluginData() {
    return $.extend(true, {}, {
        ports: {
            control: {
                input: []
            },
        },
    })
}

// Map a StoreItem from /store/{source}/search to a plugin-like object
function mapStoreItem(item) {
    var cat = (item.categories && item.categories.length > 0) ? item.categories : ['Utility']
    return {
        uri: item.url || ('store:patchstorage:' + item.id),
        store_id: item.id,
        store_source: 'patchstorage',
        label: item.title,
        name: item.title,
        brand: item.author || 'Unknown',
        author: { name: item.author || 'Unknown', homepage: item.url || '' },
        comment: item.description || '',
        category: cat,
        screenshot_href: item.thumbnail_url || '/resources/pedals/default-screenshot.png',
        thumbnail_href: item.thumbnail_url || '/resources/pedals/default-thumbnail.png',
        status: 'blocked',
        installedVersion: null,
        latestVersion: [0, 0, 0, 1],
        bundle_id: null,
        bundle_name: null,
        bundles: [],
        ports: { control: { input: [] }, cv: { input: [], output: [] } },
        gui: false,
        buildEnvironment: null,
        stable: true,
    }
}

JqueryClass('cloudPluginBox', {
    init: function (options) {
        var self = $(this)

        options = $.extend({
            resultCanvas: self.find('.js-cloud-plugins'),
            removePluginBundles: function (bundles, callback) {
                callback({})
            },
            installPluginURI: function (uri, usingLabs, callback) {
                callback({}, "")
            },
            upgradePluginURI: function (uri, usingLabs, callback) {
                callback({}, "")
            },
            info: null,
            fake: false,
            isMainWindow: true,
            usingLabs: false,
            windowName: "Plugin Store",
            pluginsData: {},
        }, options)

        self.data(options)

        var searchbox = self.find('input[type=search]')

        // make sure searchbox is empty on init
        searchbox.val("")

        self.data('searchbox', searchbox)
        searchbox.cleanableInput()

        self.data('category', null)
        self.data('storePage', 1)
        self.data('storeTotalPages', 1)
        self.data('storeTotal', 0)
        self.cloudPluginBox('setCategory', "All")

        self.data('usingLabs', self.find('input:radio[name=plugins-source]:checked').val() === 'labs')

        var lastKeyTimeout = null
        searchbox.keydown(function (e) {
            if (e.keyCode == 13) { // detect enter
                if (lastKeyTimeout != null) {
                    clearTimeout(lastKeyTimeout)
                    lastKeyTimeout = null
                }
                self.cloudPluginBox('search')
                return false
            }
            else if (e.keyCode == 8 || e.keyCode == 46) { // detect delete and backspace
                if (lastKeyTimeout != null) {
                    clearTimeout(lastKeyTimeout)
                }
                lastKeyTimeout = setTimeout(function () {
                    self.cloudPluginBox('search')
                }, 400);
            }
        })
        searchbox.keypress(function (e) { // keypress won't detect delete and backspace but will only allow inputable keys
            if (e.which == 13)
                return
            if (lastKeyTimeout != null) {
                clearTimeout(lastKeyTimeout)
            }
            lastKeyTimeout = setTimeout(function () {
                self.cloudPluginBox('search')
            }, 400);
        })
        searchbox.on('cut', function(e) {
            if (lastKeyTimeout != null) {
                clearTimeout(lastKeyTimeout)
            }
            lastKeyTimeout = setTimeout(function () {
                self.cloudPluginBox('search')
            }, 400);
        })
        searchbox.on('paste', function(e) {
            if (lastKeyTimeout != null) {
                clearTimeout(lastKeyTimeout)
            }
            lastKeyTimeout = setTimeout(function () {
                self.cloudPluginBox('search')
            }, 400);
        })

        self.find('input:checkbox[name=installed]').click(function (e) {
            self.find('input:checkbox[name=non-installed]').prop('checked', false)
            self.cloudPluginBox('search')
        })
        self.find('input:checkbox[name=non-installed]').click(function (e) {
            self.find('input:checkbox[name=installed]').prop('checked', false)
            self.cloudPluginBox('search')
        })

        self.find('input:radio[name=plugins-source]').click(function (e) {
            self.data('usingLabs', self.find('input:radio[name=plugins-source]:checked').val() === 'labs')
            self.cloudPluginBox('toggleFeaturedPlugins')
            self.cloudPluginBox('search')
        })

        $('#cloud_install_all').click(function (e) {
            if (! $(this).hasClass("disabled")) {
                $(this).addClass("disabled").css({color:'#444'})
                self.cloudPluginBox('installAllPlugins', false)
            }
        })
        $('#cloud_update_all').click(function (e) {
            if (! $(this).hasClass("disabled")) {
                $(this).addClass("disabled").css({color:'#444'})
                self.cloudPluginBox('installAllPlugins', true)
            }
        })

        var results = {}
        self.data('results', results)

        self.data('firstLoad', true)
        self.data('categoryCounts', null)
        self.find('ul.categories li').click(function () {
            var category = $(this).attr('id').replace(/^cloud-plugin-tab-/, '')
            self.cloudPluginBox('setCategory', category)
            self.cloudPluginBox('search')
        })

        options.open = function () {
            self.data('firstLoad', true)
            $('#cloud_install_all').addClass("disabled").css({color:'#444'})
            $('#cloud_update_all').addClass("disabled").css({color:'#444'})

            self.cloudPluginBox('search')

            return false
        }

        self.window(options)

        return self
    },

    setCategory: function (category) {
        var self = $(this)

        self.find('ul.categories li').removeClass('selected')
        self.find('.plugins-wrapper').hide()
        self.find('#cloud-plugin-tab-' + category).addClass('selected')
        self.find('#cloud-plugin-content-' + category).show().css('display', 'inline-block')
        self.data('category', category)
    },
    cleanResults: function () {
        var self = $(this)
        self.find('.plugins-wrapper').html('')
        self.find('.store-pagination').remove()
        self.find('ul.categories li').each(function () {
            var content = $(this).html().split(/\s/)
            if (content.length >= 2 && content[1] == "Utility") {
                $(this).html(content[0] + " Utility")
            } else {
                $(this).html(content[0])
            }
        });
    },
    checkLocalScreenshot: function (plugin) {
        if (plugin.status == 'installed') {
            if (plugin.gui && plugin.gui.screenshot) {
                var uri = escape(plugin.uri)
                var ver = plugin.installedVersion.join('_')
                plugin.screenshot_href = "/effect/image/screenshot.png?uri=" + uri + "&v=" + ver
                plugin.thumbnail_href  = "/effect/image/thumbnail.png?uri=" + uri + "&v=" + ver
            } else {
                plugin.screenshot_href = "/resources/pedals/default-screenshot.png"
                plugin.thumbnail_href  = "/resources/pedals/default-thumbnail.png"
            }
        }
        else {
            //if (!plugin.screenshot_available && !plugin.thumbnail_available) {
            if (!plugin.screenshot_href && !plugin.thumbnail_href) {
                plugin.screenshot_href = "/resources/pedals/default-screenshot.png"
                plugin.thumbnail_href  = "/resources/pedals/default-thumbnail.png"
            }
        }
    },

    toggleFeaturedPlugins: function () {
    },

    // search all or installed, depending on selected option
    search: function (customRenderCallback) {
        var self  = $(this)
        self.data('storePage', 1)
        self.cloudPluginBox('searchWithPage', customRenderCallback)
    },

    searchPage: function (page) {
        var self = $(this)
        self.data('storePage', page)
        self.cloudPluginBox('searchWithPage', null)
    },

    searchWithPage: function (customRenderCallback) {
        var self  = $(this)
        var query = {
            text: self.data('searchbox').val(),
            summary: "true",
            image_version: VERSION,
            bin_compat: BIN_COMPAT,
        }

        if (self.data('fake')) {
            query.stable = true
        }

        // invalidate category counts when search text changes
        var lastSearchText = self.data('lastSearchText') || ''
        if ((query.text || '') !== lastSearchText) {
            self.data('categoryCounts', null)
            self.data('lastSearchText', query.text || '')
        }

        var usingLabs = self.data('usingLabs')

        if (self.find('input:checkbox[name=installed]:checked').length)
            return self.cloudPluginBox('searchInstalled', usingLabs, query, customRenderCallback)

        if (self.find('input:checkbox[name=non-installed]:checked').length)
            return self.cloudPluginBox('searchAll', usingLabs, false, query, customRenderCallback)

        return self.cloudPluginBox('searchAll', usingLabs, true, query, customRenderCallback)
    },

    synchronizePluginData: function (plugin) {
        var index = $(this).data('pluginsData')
        indexed = index[plugin.uri]
        if (indexed == null) {
            indexed = {}
            index[plugin.uri] = indexed
        }
        // Let's store all data safely, while modifying the given object
        // to have all available data
        $.extend(indexed, plugin)
        $.extend(plugin, indexed)

        if (window.devicePixelRatio && window.devicePixelRatio >= 2) {
            plugin.thumbnail_href = plugin.thumbnail_href.replace("thumbnail","screenshot")
        }
    },

    rebuildSearchIndex: function () {
        var plugins = Object.values($(this).data('pluginsData'))
        desktop.resetPluginIndexer(plugins.filter(function(plugin) { return !!plugin.installedVersion }))
    },

    // search cloud and local plugins, prefer cloud
    searchAll: function (usingLabs, showInstalled, query, customRenderCallback) {
        var self = $(this)
        var results = {}
        var cplugin, lplugin,
            cloudReached = false

        renderResults = function () {
            if (results.local == null || results.cloud == null)
                return

            var plugins = []

            for (var i in results.cloud) {
                cplugin = results.cloud[i]
                lplugin = results.local[cplugin.uri]

                if (!showInstalled && lplugin) {
                    continue
                }


                if (results.featured) {
                    cplugin.featured = results.featured.filter(function (ft) { return ft.uri === cplugin.uri })[0]
                }

                cplugin.latestVersion = [cplugin.builder_version || 0, cplugin.minorVersion, cplugin.microVersion, cplugin.release_number]

                if (lplugin) {
                    if (!lplugin.installedVersion) {
                        console.log("local plugin is missing version info:", lplugin.uri)
                        lplugin.installedVersion = [0, 0, 0, 0]
                    }

                    cplugin.installedVersion = lplugin.installedVersion
                    delete results.local[cplugin.uri]

                    if (compareVersions(cplugin.installedVersion, cplugin.latestVersion) >= 0) {
                        cplugin.status = 'installed'
                    } else {
                        cplugin.status = 'outdated'
                    }

                    // overwrite build environment if local plugin
                    cplugin.buildEnvironment = lplugin.buildEnvironment

                    self.cloudPluginBox('checkLocalScreenshot', cplugin)

                } else {
                    cplugin.installedVersion = null // if set to [0, 0, 0, 0], it appears as intalled on cloudplugininfo
                    cplugin.status = 'blocked'
                }

                if (self.data('fake') && cplugin.mod_license === 'paid_perpetual') {
                    cplugin.licensed = true;
                }

                if (!cplugin.screenshot_available && !cplugin.thumbnail_available) {
                    if (!cplugin.screenshot_href && !cplugin.thumbnail_href) {
                        cplugin.screenshot_href = "/resources/pedals/default-screenshot.png"
                        cplugin.thumbnail_href  = "/resources/pedals/default-thumbnail.png"
                    }
                }
                self.cloudPluginBox('synchronizePluginData', cplugin)
                plugins.push(cplugin)
            }

            // for all the other plugins that are not in the cloud
            if (showInstalled) {
                for (var uri in results.local) {
                    lplugin = results.local[uri]
                    lplugin.status = 'installed'
                    lplugin.latestVersion = null
                    self.cloudPluginBox('checkLocalScreenshot', lplugin)
                    self.cloudPluginBox('synchronizePluginData', lplugin)
                    plugins.push(lplugin)
                }
            }

            if (customRenderCallback) {
                customRenderCallback(plugins)
            } else {
                self.cloudPluginBox('showPlugins', plugins, cloudReached)
            }

            if (self.data('firstLoad')) {
                self.data('firstLoad', false)
                $('#cloud_install_all').removeClass("disabled").css({color:'white'})
                $('#cloud_update_all').removeClass("disabled").css({color:'white'})
            }
            self.cloudPluginBox('rebuildSearchIndex')
        }

        // store search (Patchstorage)
        var cloudResults
        var storePage = self.data('storePage') || 1
        var selectedCategory = self.data('category')
        var searchData = { q: query.text || '', page: storePage, per_page: 24 }
        if (selectedCategory && selectedCategory !== 'All') {
            searchData.category = selectedCategory
        }
        $.ajax({
            method: 'GET',
            url: '/store/patchstorage/search',
            data: searchData,
            success: function (result) {
                if (result.items) {
                    cloudReached = true
                    cloudResults = result.items.map(mapStoreItem)
                    self.data('storePage', result.page || 1)
                    self.data('storeTotalPages', result.total_pages || 1)
                    self.data('storeTotal', result.total || 0)
                } else {
                    cloudResults = []
                }
            },
            error: function () {
                cloudResults = []
            },
            complete: function () {
                results.cloud = cloudResults
                results.featured = []
                renderResults()
            },
            cache: false,
            dataType: 'json'
        })

        if (self.data('fake')) {
            results.local = {}
            renderResults()
            return;
        }

        // local search
        if (query.text)
        {
            var lplugins = {}

            var ret = desktop.pluginIndexer.search(query.text)
            for (var i in ret) {
                var uri = ret[i].ref
                var pluginData = self.data('pluginsData')[uri]
                if (! pluginData) {
                    console.log("ERROR: Plugin '" + uri + "' was not previously cached, cannot show it")
                    continue
                }
                lplugins[uri] = pluginData
            }

            results.local = $.extend(true, {}, lplugins) // deep copy instead of link/reference
            renderResults()
        }
        else
        {
            $.ajax({
                method: 'GET',
                url: '/effect/list',
                success: function (plugins) {
                    var i, plugin, allplugins = {}
                    for (i in plugins) {
                        plugin = plugins[i]

                        plugin.installedVersion = [plugin.builder, plugin.minorVersion, plugin.microVersion, plugin.release]
                        allplugins[plugin.uri] = plugin
                    }

                    results.local = $.extend(true, {}, allplugins) // deep copy instead of link/reference
                    renderResults()
                },
                error: function () {
                    results.local = {}
                    renderResults()
                },
                cache: false,
                dataType: 'json'
            })
        }
    },

    // search cloud and local plugins, show installed only
    searchInstalled: function (usingLabs, query, customRenderCallback) {
        var self = $(this)
        var results = {}
        var cplugin, lplugin,
            cloudReached = false

        renderResults = function () {
            var plugins = []

            for (var i in results.local) {
                lplugin = results.local[i]
                cplugin = results.cloud[lplugin.uri]

                if (!lplugin.installedVersion) {
                    console.log("local plugin is missing version info:", lplugin.uri)
                    lplugin.installedVersion = [0, 0, 0, 0]
                }

                if (cplugin) {
                    lplugin.latestVersion = [cplugin.builder_version || 0, cplugin.minorVersion, cplugin.microVersion, cplugin.release_number]

                    if (compareVersions(lplugin.installedVersion, lplugin.latestVersion) >= 0) {
                        lplugin.status = 'installed'
                    } else {
                        lplugin.status = 'outdated'
                    }
                } else {
                    lplugin.latestVersion = null
                    lplugin.status = 'installed'
                }

                // we're showing installed only, so prefer to show installed modgui screenshot
                if (lplugin.gui && lplugin.gui.screenshot) {
                    var uri = escape(lplugin.uri)
                    var ver = [lplugin.builder, lplugin.microVersion, lplugin.minorVersion, lplugin.release].join('_')

                    lplugin.screenshot_href = "/effect/image/screenshot.png?uri=" + uri + "&v=" + ver
                    lplugin.thumbnail_href  = "/effect/image/thumbnail.png?uri=" + uri + "&v=" + ver
                } else {
                    lplugin.screenshot_href = "/resources/pedals/default-screenshot.png"
                    lplugin.thumbnail_href  = "/resources/pedals/default-thumbnail.png"
                }
                self.cloudPluginBox('synchronizePluginData', lplugin)
                plugins.push(lplugin)
            }

            if (customRenderCallback) {
                customRenderCallback(plugins)
            } else {
                self.cloudPluginBox('showPlugins', plugins, cloudReached)
            }

            if (self.data('firstLoad')) {
                self.data('firstLoad', false)
                $('#cloud_install_all').removeClass("disabled").css({color:'white'})
                $('#cloud_update_all').removeClass("disabled").css({color:'white'})
            }
            self.cloudPluginBox('rebuildSearchIndex')
        }

        // no cloud version check for installed-only view (store uses different IDs)
        results.cloud = {}
        if (results.local != null)
            renderResults()

        // local search
        if (query.text)
        {
            var lplugins = []

            var ret = desktop.pluginIndexer.search(query.text)
            for (var i in ret) {
                var uri = ret[i].ref
                var pluginData = self.data('pluginsData')[uri]
                if (! pluginData) {
                    console.log("ERROR: Plugin '" + uri + "' was not previously cached, cannot show it")
                    continue
                }
                lplugins.push(pluginData)
            }

            results.local = $.extend(true, {}, lplugins) // deep copy instead of link/reference
            if (results.cloud != null)
                renderResults()
        }
        else
        {
            $.ajax({
                method: 'GET',
                url: '/effect/list',
                success: function (plugins) {
                    var i, plugin
                    for (i in plugins) {
                        plugin = plugins[i]
                        plugin.installedVersion = [plugin.builder || 0, plugin.minorVersion, plugin.microVersion, plugin.release]
                    }

                    results.local = plugins
                    if (results.cloud != null)
                        renderResults()
                },
                cache: false,
                dataType: 'json'
            })
        }
    },

    showPlugins: function (plugins, cloudReached) {
        var self = $(this)
        self.cloudPluginBox('cleanResults')

        // sort plugins by label
        plugins.sort(function (a, b) {
            a = a.label.toLowerCase()
            b = b.label.toLowerCase()
            if (a > b) {
                return 1
            }
            if (a < b) {
                return -1
            }
            return 0
        })

        var selectedCategory = self.data('category') || 'All'
        var contentCanvas = self.find('#cloud-plugin-content-' + selectedCategory)
        if (contentCanvas.length === 0) {
            contentCanvas = self.find('#cloud-plugin-content-All')
        }
        var pluginsDict = {}

        var plugin, render
        for (var i in plugins) {
            plugin = plugins[i]
            render = self.cloudPluginBox('renderPlugin', plugin, cloudReached)
            pluginsDict[plugin.uri] = plugin
            render.appendTo(contentCanvas)
        }

        self.data('pluginsDict', pluginsDict)

        // fetch and display category counts (once on first load)
        self.cloudPluginBox('fetchCategoryCounts')

        // render pagination controls
        self.cloudPluginBox('renderPagination')
    },

    renderPagination: function () {
        var self = $(this)
        var page = self.data('storePage') || 1
        var totalPages = self.data('storeTotalPages') || 1
        var total = self.data('storeTotal') || 0

        self.find('.store-pagination').remove()

        if (totalPages <= 1) return

        var nav = $('<div class="store-pagination"></div>')

        var prevBtn = $('<button class="btn btn-mini">&laquo; Prev</button>')
        if (page <= 1) {
            prevBtn.prop('disabled', true).css('opacity', 0.4)
        } else {
            prevBtn.click(function () { self.cloudPluginBox('searchPage', page - 1) })
        }

        var nextBtn = $('<button class="btn btn-mini">Next &raquo;</button>')
        if (page >= totalPages) {
            nextBtn.prop('disabled', true).css('opacity', 0.4)
        } else {
            nextBtn.click(function () { self.cloudPluginBox('searchPage', page + 1) })
        }

        var info = $('<span style="margin:0 15px;color:#ccc;">Page ' + page + ' of ' + totalPages + ' (' + total + ' plugins)</span>')

        nav.append(prevBtn, info, nextBtn)
        nav.css({ textAlign: 'center', padding: '15px 0', clear: 'both' })
        var selectedCategory = self.data('category') || 'All'
        var contentDiv = self.find('#cloud-plugin-content-' + selectedCategory)
        if (contentDiv.length === 0) {
            contentDiv = self.find('#cloud-plugin-content-All')
        }
        contentDiv.after(nav)
    },

    fetchCategoryCounts: function () {
        var self = $(this)
        var counts = self.data('categoryCounts')
        var searchText = self.data('searchbox').val() || ''

        // Use storeTotal for the currently selected category
        var selectedCategory = self.data('category') || 'All'
        var storeTotal = self.data('storeTotal') || 0

        if (counts) {
            // Update the count for the current category from the API total
            counts[selectedCategory] = storeTotal
            self.cloudPluginBox('setCategoryCount', counts)
            return
        }

        // First load: fetch counts for all categories in parallel
        counts = {}
        var categoryTabs = []
        self.find('ul.categories li').each(function () {
            var catId = $(this).attr('id').replace(/^cloud-plugin-tab-/, '')
            categoryTabs.push(catId)
        })

        var pending = categoryTabs.length
        var done = function () {
            pending--
            if (pending <= 0) {
                self.data('categoryCounts', counts)
                self.cloudPluginBox('setCategoryCount', counts)
            }
        }

        for (var i = 0; i < categoryTabs.length; i++) {
            (function (cat) {
                var data = { q: searchText, page: 1, per_page: 1 }
                if (cat !== 'All') {
                    data.category = cat
                }
                $.ajax({
                    method: 'GET',
                    url: '/store/patchstorage/search',
                    data: data,
                    success: function (result) {
                        counts[cat] = result.total || 0
                    },
                    error: function () {
                        counts[cat] = 0
                    },
                    complete: done,
                    cache: false,
                    dataType: 'json'
                })
            })(categoryTabs[i])
        }
    },

    setCategoryCount: function (categories) {
        var self = $(this)
        self.data('categoryCount', categories)

        for (var category in categories) {
            var tab = self.find('#cloud-plugin-tab-' + category)
            if (tab.length == 0) {
                continue
            }
            var content = tab.html().split(/\s/)

            if (content.length >= 2 && content[1] == "Utility") {
                content = content[0] + " Utility"
            } else {
                content = content[0]
            }
            tab.html(content + ' <span class="plugin_count">(' + categories[category] + ')</span>')
        }
    },

    renderPlugin: function (plugin, cloudReached, featured) {
        var self = $(this)
        var uri = escape(plugin.uri)
        var comment = plugin.comment.trim()
        var has_comment = ""
        if(!comment) {
            comment = "No description available";
            has_comment = "no_description";
        }
        var plugin_data = {
            uri: uri,
            screenshot_href: plugin.screenshot_href,
            thumbnail_href: plugin.thumbnail_href,
            has_comment: has_comment,
            comment: comment,
            status: plugin.status,
            brand : plugin.brand,
            label : plugin.label,
            build_env: plugin.buildEnvironment,
        }

        var template = featured ? TEMPLATES.featuredplugin : TEMPLATES.cloudplugin
        var rendered = $(Mustache.render(template, plugin_data))
        rendered.click(function () {
            self.cloudPluginBox('showPluginInfo', plugin.uri)
        })

        return rendered
    },

    installAllPlugins: function (updateOnly) {
        var self = $(this)

        self.cloudPluginBox('search', function (plugins) {
            // sort plugins by label
            var alower, blower
            plugins.sort(function (a, b) {
                alower = a.label.toLowerCase()
                blower = b.label.toLowerCase()
                if (alower > blower)
                    return 1
                if (alower < blower)
                    return -1
                return 0
            })

            var bundle_id, bundle_ids = []
            var currentCategory = $("#cloud-plugins-library .categories .selected").attr('id').replace(/^cloud-plugin-tab-/, '') || "All"

            var plugin
            for (var i in plugins) {
                plugin = plugins[i]
                if (! plugin.bundle_id || ! plugin.latestVersion) {
                    continue
                }
                if (plugin.installedVersion) {
                    if (compareVersions(plugin.latestVersion, plugin.installedVersion) <= 0) {
                        continue
                    }
                } else if (updateOnly) {
                    continue
                }

                var category = plugin.category[0]
                if (category == 'Utility' && plugin.category.length == 2 && plugin.category[1] == 'MIDI') {
                    category = 'MIDI'
                }

                // FIXME for midi
                if (bundle_ids.indexOf(plugin.bundle_id) < 0 && (currentCategory == "All" || currentCategory == category)) {
                    bundle_ids.push(plugin.bundle_id)
                }
            }

            if (bundle_ids.length == 0) {
                $('#cloud_install_all').removeClass("disabled").css({color:'white'})
                $('#cloud_update_all').removeClass("disabled").css({color:'white'})
                new Notification('warn', 'All plugins are '+(updateOnly?'updated':'installed')+', nothing to do', 8000)
                return
            }

            var count = 0
            var finished = function (resp, bundlename) {
                self.cloudPluginBox('postInstallAction', resp.installed, resp.removed, bundlename)
                count += 1
                if (count == bundle_ids.length) {
                    $('#cloud_install_all').removeClass("disabled").css({color:'white'})
                    $('#cloud_update_all').removeClass("disabled").css({color:'white'})
                    new Notification('warn', 'All plugins are now '+(updateOnly?'updated':'installed'), 8000)
                }
                if (resp.ok) {
                    self.cloudPluginBox('search')
                }
            }
            var usingLabs = self.data('usingLabs')

            for (var i in bundle_ids) {
                desktop.installationQueue.installUsingBundle(bundle_ids[i], usingLabs, finished)
            }
        })
    },

    postInstallAction: function (installed, removed, bundlename) {
        var self = $(this)
        var bundle = LV2_PLUGIN_DIR + bundlename
        var category, categories = self.data('categoryCount')
        var uri, plugin, oldElem, newElem

        for (var i in installed) {
            uri    = installed[i]
            plugin = self.data('pluginsData')[uri]

            if (! plugin) {
                continue
            }

            plugin.status  = 'installed'
            plugin.bundles = [bundle]
            plugin.installedVersion = plugin.latestVersion

            oldElem = self.find('.cloud-plugin[mod-uri="'+escape(uri)+'"]')
            newElem = self.cloudPluginBox('renderPlugin', plugin, true)
            oldElem.replaceWith(newElem)
        }

        for (var i in removed) {
            uri = removed[i]

            if (installed.indexOf(uri) >= 0) {
                continue
            }

            var favoriteIndex = FAVORITES.indexOf(uri)
            if (favoriteIndex >= 0) {
                FAVORITES.splice(favoriteIndex, 1)
                $('#effect-content-Favorites').find('[mod-uri="'+escape(uri)+'"]').remove()
                $('#effect-tab-Favorites').html('Favorites (' + FAVORITES.length + ')')
            }

            plugin  = self.data('pluginsData')[uri]
            oldElem = self.find('.cloud-plugin[mod-uri="'+escape(uri)+'"]')

            if (plugin.latestVersion) {
                // removing a plugin available on cloud, keep its store item
                plugin.status = 'blocked'
                plugin.bundle_name = bundle
                delete plugin.bundles
                plugin.installedVersion = null

                newElem = self.cloudPluginBox('renderPlugin', plugin, true)
                oldElem.replaceWith(newElem)

            } else {
                // removing local plugin means the number of possible plugins goes down
                category = plugin.category[0]

                if (category && category != 'All') {
                    if (category == 'Utility' && plugin.category.length == 2 && plugin.category[1] == 'MIDI') {
                        category = 'MIDI'
                    }
                    categories[category] -= 1
                }
                categories['All'] -= 1

                // remove it from store
                delete self.data('pluginsData')[uri]
                oldElem.remove()
            }
        }

        self.cloudPluginBox('setCategoryCount', categories)
    },

    showPluginInfo: function (uri) {
        var self = $(this)

        var plugin = self.data('pluginsData')[uri]
        if (!plugin) {
            if (self.data('fake'))
                new Notification('error', "Requested plugin is not available")
            return
        }

        var cloudChecked = false
        var localChecked = false

        var showInfo = function() {
            if (!cloudChecked || !localChecked)
                return

            // formating numbers and flooring ranges up to two decimal cases
            for (var i = 0; i < plugin.ports.control.input.length; i++) {
                plugin.ports.control.input[i].formatted = format(plugin.ports.control.input[i])
            }

            if (plugin.ports.cv && plugin.ports.cv.input) {
              for (var i = 0; i < plugin.ports.cv.input.length; i++) {
                plugin.ports.cv.input[i].formatted = format(plugin.ports.cv.input[i])
              }
            }

            if (plugin.ports.cv && plugin.ports.cv.output) {
              for (var i = 0; i < plugin.ports.cv.output.length; i++) {
                plugin.ports.cv.output[i].formatted = format(plugin.ports.cv.output[i])
              }
            }

            var category = plugin.category[0]
            if (category == 'Utility' && plugin.category.length == 2 && plugin.category[1] == 'MIDI') {
                category = 'MIDI'
            }

            var metadata = {
                author: plugin.author,
                uri: plugin.uri,
                escaped_uri: escape(plugin.uri),
                thumbnail_href: plugin.thumbnail_href,
                screenshot_href: plugin.screenshot_href,
                category: category || "None",
                installed_version: version(plugin.installedVersion),
                latest_version: version(plugin.latestVersion),
                package_name: (plugin.bundle_name || (plugin.bundles && plugin.bundles[0]) || plugin.label || '').replace(/\.lv2$/, ''),
                comment: plugin.comment.trim() || "No description available",
                brand : plugin.brand,
                name  : plugin.name,
                label : plugin.label,
                ports : plugin.ports,
                plugin_href: plugin.store_id ? plugin.uri : (PLUGINS_URL + '/' + btoa(plugin.uri)),
                pedalboard_href: plugin.store_id ? null : desktop.getPedalboardHref(plugin.uri, plugin.stable === false),
                build_env_uppercase: (plugin.buildEnvironment || "LOCAL").toUpperCase(),
                show_build_env: plugin.buildEnvironment !== "prod",
            };

            var info = self.data('info')

            if (info) {
                info.remove()
                self.data('info', null)
            }
            info = $(Mustache.render(TEMPLATES.cloudplugin_info, metadata))

            // hide control ports table if none available
            if (plugin.ports.control.input.length == 0) {
                info.find('.plugin-controlports').hide()
            }

            // hide cv inputs table if none available
            if (!plugin.ports.cv || (plugin.ports.cv && plugin.ports.cv.input && plugin.ports.cv.input.length == 0)) {
                info.find('.plugin-cvinputs').hide()
            }

            // hide cv ouputs ports table if none available
            if (!plugin.ports.cv || (plugin.ports.cv && plugin.ports.cv.output && plugin.ports.cv.output.length == 0)) {
                info.find('.plugin-cvoutputs').hide()
            }

            var canInstall = false,
                canUpgrade = false

            // The remove button will remove the plugin, close window and re-render the plugins
            // without the removed one
            if (plugin.installedVersion) {
                info.find('.js-install').hide()
                info.find('.js-remove').show().click(function () {
                    // Remove plugin
                    self.data('removePluginBundles')(plugin.bundles, function (resp) {
                        var bundlename = plugin.bundles[0].split('/').filter(function(el){return el.length!=0}).pop(0)
                        self.cloudPluginBox('postInstallAction', [], resp.removed, bundlename)
                        info.window('close')

                        // remove-only action, need to manually update plugins
                        if (desktop.updatePluginList) {
                            desktop.updatePluginList([], resp.removed)
                        }
                    })
                })
            } else if (plugin.store_id) {
                // Store plugin - install directly via store endpoint
                canInstall = true
                info.find('.js-remove').hide()
                info.find('.js-installed-version').hide()
                info.find('.js-install').show().click(function () {
                    var installBtn = info.find('.js-install')
                    installBtn.prop('disabled', true).text('Installing...')
                    $.ajax({
                        url: '/store/' + plugin.store_source + '/install/' + plugin.store_id,
                        type: 'POST',
                        success: function (resp) {
                            if (resp.ok) {
                                new Notification('success', plugin.label + ' installed successfully', 5000)
                                self.cloudPluginBox('postInstallAction', resp.installed, resp.removed, '')
                                if (desktop.updateAllPlugins) {
                                    desktop.updateAllPlugins()
                                }
                            } else {
                                new Notification('error', 'Install failed: ' + resp.error, 8000)
                                installBtn.prop('disabled', false).text('Install')
                            }
                            info.window('close')
                        },
                        error: function () {
                            new Notification('error', 'Install failed: server error', 5000)
                            installBtn.prop('disabled', false).text('Install')
                        },
                        cache: false,
                        dataType: 'json'
                    })
                })
            } else {
                canInstall = true
                info.find('.js-remove').hide()
                info.find('.js-installed-version').hide()
                info.find('.js-install').show().click(function () {
                    // Install plugin via legacy flow
                    self.data('installPluginURI')(plugin.uri, self.data('usingLabs'), function (resp, bundlename) {
                        self.cloudPluginBox('postInstallAction', resp.installed, resp.removed, bundlename)
                        info.window('close')
                    })
                })
            }

            info.find('.js-upgrade').hide()

            if (! plugin.latestVersion) {
                info.find('.js-latest-version').hide()
            }

            info.appendTo($('body'))
            info.window({
                windowName: "Cloud Plugin Info"
            })
            info.window('open')
            self.data('info', info)
        }

        // get full plugin info if plugin has a local version
        if ((plugin.bundles && plugin.bundles.length > 0) || ! plugin.installedVersion) {
            localChecked = true
        } else {
            var renderedVersion = [plugin.builder,
                                   plugin.microVersion,
                                   plugin.minorVersion,
                                   plugin.release].join('_');
            $.ajax({
                url: "/effect/get",
                data: {
                    uri: plugin.uri,
                    version: VERSION,
                    plugin_version: renderedVersion,
                },
                success: function (pluginData) {
                    // delete cloud specific fields just in case
                    delete pluginData.bundle_name
                    delete pluginData.latestVersion
                    // ready to merge
                    plugin = $.extend(pluginData, plugin)
                    localChecked = true
                    showInfo()
                },
                error: function () {
                    // assume not installed
                    plugin.installedVersion = null
                    plugin.installed_version = null
                    localChecked = true
                    showInfo()
                },
                cache: !!plugin.buildEnvironment,
                dataType: 'json'
            })
        }

        // get store or cloud plugin info
        if (plugin.store_id) {
            // Store plugin - fetch details from our store endpoint
            $.ajax({
                url: '/store/' + (plugin.store_source || 'patchstorage') + '/get/' + plugin.store_id,
                success: function (item) {
                    if (item && !item.error) {
                        var mapped = mapStoreItem(item)
                        plugin = $.extend(mapped, plugin)
                    } else {
                        plugin = $.extend(getDummyPluginData(), plugin)
                    }
                    plugin.latestVersion = null
                    cloudChecked = true
                    showInfo()
                },
                error: function () {
                    plugin = $.extend(getDummyPluginData(), plugin)
                    plugin.latestVersion = null
                    cloudChecked = true
                    showInfo()
                },
                cache: false,
                dataType: 'json'
            })
        } else {
            // Local-only plugin, no cloud info available
            plugin = $.extend(getDummyPluginData(), plugin)
            plugin.latestVersion = null
            cloudChecked = true
            showInfo()
        }
    },
})
