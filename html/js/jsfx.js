// JSFX store panel — browse and install JSFX effects from ReaPack repositories.

var jsfx = {
    page: 1,
    totalPages: 1,
    total: 0,
    category: '',
    searchText: '',
    searchTimeout: null,

    init: function () {
        var self = this;

        // Category clicks
        $('#jsfx-categories').on('click', 'li', function () {
            $('#jsfx-categories li').removeClass('selected');
            $(this).addClass('selected');
            var cat = $(this).attr('id').replace('jsfx-tab-', '');
            self.category = (cat === 'All') ? '' : cat;
            self.page = 1;
            self.search();
        });

        // Search input
        var searchBox = $('#jsfx-search');
        searchBox.keydown(function (e) {
            if (e.keyCode == 13) {
                if (self.searchTimeout) clearTimeout(self.searchTimeout);
                self.searchText = searchBox.val();
                self.page = 1;
                self.search();
                return false;
            }
        });
        searchBox.on('input', function () {
            if (self.searchTimeout) clearTimeout(self.searchTimeout);
            self.searchTimeout = setTimeout(function () {
                self.searchText = searchBox.val();
                self.page = 1;
                self.search();
            }, 400);
        });

        // Repo buttons
        $('#jsfx-add-repo-btn').click(function () { self.addRepo(); });
        $('#jsfx-refresh-btn').click(function () { self.refreshRepos(); });

        // Lazy load: fetch when JSFX tab is first selected
        $('#store-source-select').change(function () {
            if ($(this).val() === 'jsfx' && !self._loaded) {
                self._loaded = true;
                self.loadCategories();
                self.loadRepos();
                self.search();
            }
        });
    },

    loadCategories: function () {
        $.get('/store/jsfx/categories', function (cats) {
            if (!cats || cats.error) return;
            var ul = $('#jsfx-categories');
            ul.find('li:not(#jsfx-tab-All)').remove();
            for (var i = 0; i < cats.length; i++) {
                var c = cats[i];
                ul.append('<li id="jsfx-tab-' + c.slug + '">' + c.name + '</li>');
            }
        });
    },

    loadRepos: function () {
        var self = this;
        $.get('/store/jsfx/repos', function (resp) {
            if (!resp || !resp.repos) return;
            self.renderRepos(resp.repos);
        });
    },

    renderRepos: function (repos) {
        var self = this;
        var container = $('#jsfx-repos-list');
        container.html('');
        for (var i = 0; i < repos.length; i++) {
            (function (idx, repo) {
                var row = $('<div style="display:flex;align-items:center;padding:3px 0;"></div>');
                var toggle = $('<input type="checkbox" style="margin-right:6px;">');
                toggle.prop('checked', repo.enabled);
                toggle.change(function () { self.toggleRepo(idx); });
                var name = $('<span style="flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:' + (repo.enabled ? '#ccc' : '#666') + ';">' + repo.name + '</span>');
                var del = $('<span style="cursor:pointer;color:#f66;margin-left:6px;font-size:14px;" title="Remove">&times;</span>');
                del.click(function () { self.removeRepo(idx); });
                row.append(toggle, name, del);
                container.append(row);
            })(i, repos[i]);
        }
    },

    search: function () {
        var self = this;
        var data = { q: self.searchText, page: self.page, per_page: 24 };
        if (self.category) data.category = self.category;

        $.ajax({
            method: 'GET',
            url: '/store/jsfx/search',
            data: data,
            success: function (result) {
                if (result.error) {
                    self.showError(result.error);
                    return;
                }
                self.page = result.page || 1;
                self.totalPages = result.total_pages || 1;
                self.total = result.total || 0;
                self.renderResults(result.items || []);
            },
            error: function () {
                self.showError('Failed to search JSFX effects');
            },
            dataType: 'json'
        });
    },

    showError: function (msg) {
        $('#jsfx-content').html('<p style="color:#f66;padding:20px;">' + msg + '</p>');
    },

    renderResults: function (items) {
        var self = this;
        var content = $('#jsfx-content');
        content.html('');

        if (items.length === 0) {
            content.html('<p style="color:#999;padding:20px;">No results found.</p>');
            return;
        }

        for (var i = 0; i < items.length; i++) {
            content.append(self.renderCard(items[i]));
        }

        self.renderPagination();
    },

    renderCard: function (item) {
        var self = this;
        var desc = item.description || '';
        if (desc.length > 120) desc = desc.substring(0, 120) + '...';
        if (!desc) desc = 'No description';
        var cats = (item.categories || []).join(' / ');

        var card = $(
            '<div class="cloud-plugin plugin-container available-plugin">' +
                '<div class="cloud-plugin-border">' +
                    '<figure class="thumb"><img src="/resources/pedals/jsfx-thumbnail.png" onerror="this.style.display=\'none\'"></figure>' +
                    '<div class="description">' +
                        '<span class="title">' + (item.title || 'Untitled') + '</span>' +
                        '<span class="author">' + (item.author || '') +
                            (cats ? ' <span style="color:#888;font-size:11px;">(' + cats + ')</span>' : '') +
                        '</span>' +
                        '<hr class="dotted" />' +
                        '<p>' + desc + '<span class="limiter"></span></p>' +
                    '</div>' +
                '</div>' +
            '</div>'
        );

        card.click(function () { self.showDetail(item); });
        return card;
    },

    showDetail: function (item) {
        var self = this;
        var cats = (item.categories || []).join(' / ');

        var overlay = $(
            '<div id="jsfx-detail-overlay" style="position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.8);z-index:1000;overflow-y:auto;">' +
                '<div style="max-width:600px;margin:60px auto;background:#222;padding:30px;border-radius:8px;color:#ccc;">' +
                    '<h2 style="margin:0 0 10px;color:#fff;">' + (item.title || 'Untitled') + '</h2>' +
                    '<p style="color:#aaa;">by ' + (item.author || 'Unknown') + '</p>' +
                    '<p style="color:#888;font-size:12px;">' + cats + '</p>' +
                    '<p style="margin:15px 0;white-space:pre-line;">' + (item.description || 'No description available.') + '</p>' +
                    '<div style="margin-top:20px;">' +
                        '<button class="btn js-jsfx-install" style="margin-right:10px;">Install</button>' +
                        '<button class="btn js-jsfx-close">Close</button>' +
                    '</div>' +
                    '<div id="jsfx-detail-status" style="margin-top:15px;display:none;"></div>' +
                '</div>' +
            '</div>'
        );

        overlay.find('.js-jsfx-close').click(function () { overlay.remove(); });
        overlay.find('.js-jsfx-install').click(function () {
            self.installJsfx(item, $(this), overlay.find('#jsfx-detail-status'));
        });

        $('body').append(overlay);
    },

    installJsfx: function (item, btn, statusEl) {
        btn.prop('disabled', true).text('Installing...');
        statusEl.show().html('<p style="color:#aaf;">Downloading and scanning JSFX...</p>');

        $.ajax({
            method: 'POST',
            url: '/store/jsfx/install/' + item.id,
            success: function (resp) {
                if (resp.ok) {
                    var plugins = (resp.installed || []).join(', ');
                    statusEl.html('<p style="color:#6f6;">Installed: ' + (plugins || 'JSFX plugin') + '</p>');
                    btn.text('Installed').css('opacity', 0.5);
                } else {
                    statusEl.html('<p style="color:#f66;">' + (resp.error || 'Install failed') + '</p>');
                    btn.prop('disabled', false).text('Retry');
                }
            },
            error: function () {
                statusEl.html('<p style="color:#f66;">Install request failed</p>');
                btn.prop('disabled', false).text('Retry');
            },
            dataType: 'json'
        });
    },

    renderPagination: function () {
        var self = this;
        $('#jsfx-pagination').remove();
        if (self.totalPages <= 1) return;

        var nav = $('<div id="jsfx-pagination" style="text-align:center;padding:15px 0;clear:both;"></div>');

        var prevBtn = $('<button class="btn btn-mini">&laquo; Prev</button>');
        if (self.page <= 1) {
            prevBtn.prop('disabled', true).css('opacity', 0.4);
        } else {
            prevBtn.click(function () { self.page--; self.search(); });
        }

        var nextBtn = $('<button class="btn btn-mini">Next &raquo;</button>');
        if (self.page >= self.totalPages) {
            nextBtn.prop('disabled', true).css('opacity', 0.4);
        } else {
            nextBtn.click(function () { self.page++; self.search(); });
        }

        var info = $('<span style="margin:0 15px;color:#ccc;">Page ' + self.page + ' of ' + self.totalPages + ' (' + self.total + ' effects)</span>');
        nav.append(prevBtn, info, nextBtn);
        $('#jsfx-content').after(nav);
    },

    addRepo: function () {
        var self = this;
        var name = prompt('Repository name:');
        if (!name) return;
        var url = prompt('Index URL (index.xml):');
        if (!url) return;

        $.ajax({
            method: 'POST',
            url: '/store/jsfx/repos',
            contentType: 'application/json',
            data: JSON.stringify({ name: name, url: url }),
            success: function (resp) {
                if (resp.ok) {
                    self.loadRepos();
                    self.refreshRepos();
                } else {
                    alert(resp.error || 'Failed to add repo');
                }
            },
            dataType: 'json'
        });
    },

    toggleRepo: function (index) {
        var self = this;
        $.post('/store/jsfx/repos/' + index + '/toggle', function () {
            self.loadRepos();
        });
    },

    removeRepo: function (index) {
        var self = this;
        if (!confirm('Remove this repository?')) return;
        $.post('/store/jsfx/repos/' + index + '/remove', function () {
            self.loadRepos();
        });
    },

    refreshRepos: function () {
        var self = this;
        $('#jsfx-refresh-btn').prop('disabled', true).text('Refreshing...');
        $.post('/store/jsfx/repos/refresh', function () {
            $('#jsfx-refresh-btn').prop('disabled', false).text('Refresh');
            self.loadCategories();
            self.search();
        });
    },
};
