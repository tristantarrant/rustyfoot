// Tone3000 store panel logic — browse and install NAM models and impulse responses.

var tone3000 = {
    page: 1,
    totalPages: 1,
    total: 0,
    category: '',
    searchText: '',
    searchTimeout: null,

    init: function () {
        var self = this;

        // Check auth on load and on tone3000_auth query param
        var params = new URLSearchParams(window.location.search);
        if (params.get('tone3000_auth') === 'ok') {
            // Clean up URL
            window.history.replaceState({}, '', window.location.pathname);
            // Switch to tone3000 source
            $('#store-source-select').val('tone3000').trigger('change');
        }

        this.checkAuth();

        // Source selector switching
        $('#store-source-select').change(function () {
            var source = $(this).val();
            $('.store-panel').hide();
            $('#store-panel-' + source).show();
            $('[id^="store-filter-"]').hide();
            $('#store-filter-' + source).show();
        });

        // Category clicks
        $('#tone3000-categories li').click(function () {
            $('#tone3000-categories li').removeClass('selected');
            $(this).addClass('selected');
            var cat = $(this).attr('id').replace('tone3000-tab-', '');
            self.category = (cat === 'All') ? '' : cat;
            self.page = 1;
            self.search();
        });

        // Search input
        var searchBox = $('#tone3000-search');
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

        // Connect button
        $('#tone3000-connect-btn').click(function () {
            window.location.href = '/store/tone3000/auth/start';
        });

        // Disconnect button
        $('#tone3000-disconnect-btn').click(function () {
            $.post('/store/tone3000/auth/disconnect', function () {
                $('#tone3000-filter-categories').hide();
                $('#tone3000-auth-sidebar').show();
            });
        });
    },

    checkAuth: function () {
        var self = this;
        $.get('/store/tone3000/auth/status', function (resp) {
            if (resp.authenticated) {
                $('#tone3000-auth-sidebar').hide();
                $('#tone3000-filter-categories').show();
                self.search();
            } else {
                $('#tone3000-auth-sidebar').show();
                $('#tone3000-filter-categories').hide();
            }
        });
    },

    search: function () {
        var self = this;
        var data = { q: self.searchText, page: self.page, per_page: 24 };
        if (self.category) data.category = self.category;

        $.ajax({
            method: 'GET',
            url: '/store/tone3000/search',
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
                self.showError('Failed to search Tone3000');
            },
            dataType: 'json'
        });
    },

    showError: function (msg) {
        var content = $('#tone3000-content');
        content.html('<p style="color:#f66;padding:20px;">' + msg + '</p>');
    },

    renderResults: function (items) {
        var self = this;
        var content = $('#tone3000-content');
        content.html('');

        if (items.length === 0) {
            content.html('<p style="color:#999;padding:20px;">No results found.</p>');
            return;
        }

        for (var i = 0; i < items.length; i++) {
            var item = items[i];
            var card = self.renderCard(item);
            content.append(card);
        }

        self.renderPagination();
    },

    renderCard: function (item) {
        var self = this;
        var desc = item.description || '';
        if (desc.length > 120) desc = desc.substring(0, 120) + '...';
        if (!desc) desc = 'No description';

        var isIR = (item.categories || []).some(function(c) { return c.indexOf('Impulse') >= 0; });
        var defaultThumb = isIR ? '/resources/pedals/ir-thumbnail.png' : '/resources/pedals/nam-thumbnail.png';

        var card = $(
            '<div class="cloud-plugin plugin-container available-plugin">' +
                '<div class="cloud-plugin-border">' +
                    '<figure class="thumb"><img src="' + (item.thumbnail_url || defaultThumb) + '"></figure>' +
                    '<div class="description">' +
                        '<span class="title">' + (item.title || 'Untitled') + '</span>' +
                        '<span class="author">' + (item.author || '') + '</span>' +
                        '<hr class="dotted" />' +
                        '<p>' + desc + '<span class="limiter"></span></p>' +
                    '</div>' +
                '</div>' +
            '</div>'
        );

        card.click(function () {
            self.showDetail(item);
        });

        return card;
    },

    showDetail: function (item) {
        var self = this;
        var cats = (item.categories || []).join(' / ');
        var tags = (item.tags || []).join(', ');
        var downloads = item.download_count || 0;

        var overlay = $(
            '<div id="tone3000-detail-overlay" style="position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.8);z-index:1000;overflow-y:auto;">' +
                '<div style="max-width:600px;margin:60px auto;background:#222;padding:30px;border-radius:8px;color:#ccc;">' +
                    '<h2 style="margin:0 0 10px;color:#fff;">' + (item.title || 'Untitled') + '</h2>' +
                    '<p style="color:#aaa;">by ' + (item.author || 'Unknown') + '</p>' +
                    '<p style="color:#888;font-size:12px;">' + cats + (tags ? ' &middot; ' + tags : '') + ' &middot; ' + downloads + ' downloads</p>' +
                    '<p style="margin:15px 0;white-space:pre-line;">' + (item.description || 'No description available.') + '</p>' +
                    '<div id="tone3000-detail-actions" style="margin-top:20px;">' +
                        '<button class="btn js-tone3000-install" style="margin-right:10px;">Install</button>' +
                        '<button class="btn js-tone3000-close">Close</button>' +
                    '</div>' +
                    '<div id="tone3000-detail-status" style="margin-top:15px;display:none;"></div>' +
                '</div>' +
            '</div>'
        );

        overlay.find('.js-tone3000-close').click(function () {
            overlay.remove();
        });

        overlay.find('.js-tone3000-install').click(function () {
            var btn = $(this);
            btn.prop('disabled', true).text('Installing...');
            overlay.find('#tone3000-detail-status').show().html('<p style="color:#aaf;">Downloading models...</p>');

            $.ajax({
                method: 'POST',
                url: '/store/tone3000/install/' + item.id,
                contentType: 'application/json',
                data: JSON.stringify({
                    title: item.title || '',
                    categories: item.categories || [],
                }),
                success: function (resp) {
                    if (resp.ok) {
                        var files = (resp.installed || []).join(', ');
                        overlay.find('#tone3000-detail-status').html(
                            '<p style="color:#6f6;">Installed: ' + files + '</p>' +
                            '<p style="color:#999;">Saved to: ' + (resp.directory || '') + '</p>'
                        );
                        btn.text('Installed').css('opacity', 0.5);
                    } else {
                        overlay.find('#tone3000-detail-status').html('<p style="color:#f66;">' + (resp.error || 'Install failed') + '</p>');
                        btn.prop('disabled', false).text('Retry');
                    }
                },
                error: function () {
                    overlay.find('#tone3000-detail-status').html('<p style="color:#f66;">Install request failed</p>');
                    btn.prop('disabled', false).text('Retry');
                },
                dataType: 'json'
            });
        });

        $('body').append(overlay);
    },

    renderPagination: function () {
        var self = this;
        $('#tone3000-pagination').remove();
        if (self.totalPages <= 1) return;

        var nav = $('<div id="tone3000-pagination" style="text-align:center;padding:15px 0;clear:both;"></div>');

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

        var info = $('<span style="margin:0 15px;color:#ccc;">Page ' + self.page + ' of ' + self.totalPages + ' (' + self.total + ' models)</span>');
        nav.append(prevBtn, info, nextBtn);
        $('#tone3000-content').after(nav);
    },
};
