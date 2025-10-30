// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded affix "><a href="introduction.html">Introduction</a></li><li class="chapter-item expanded "><a href="quick-start.html"><strong aria-hidden="true">1.</strong> Quick Start</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="quick-start-linux.html"><strong aria-hidden="true">1.1.</strong> On Linux</a></li><li class="chapter-item expanded "><a href="quick-start-macos.html"><strong aria-hidden="true">1.2.</strong> On macOS</a></li><li class="chapter-item expanded "><a href="quick-start-windows.html"><strong aria-hidden="true">1.3.</strong> On Windows</a></li><li class="chapter-item expanded "><a href="quick-start-qgis.html"><strong aria-hidden="true">1.4.</strong> View with QGIS</a></li></ol></li><li class="chapter-item expanded "><a href="installation.html"><strong aria-hidden="true">2.</strong> Installation</a></li><li class="chapter-item expanded "><a href="run.html"><strong aria-hidden="true">3.</strong> Running</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="run-with-cli.html"><strong aria-hidden="true">3.1.</strong> Command Line Interface</a></li><li class="chapter-item expanded "><a href="env-vars.html"><strong aria-hidden="true">3.2.</strong> Environment Variables</a></li><li class="chapter-item expanded "><a href="run-hosting-environment.html"><strong aria-hidden="true">3.3.</strong> Hosting Environment–specific Guides</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="run-with-docker.html"><strong aria-hidden="true">3.3.1.</strong> Docker</a></li><li class="chapter-item expanded "><a href="run-with-docker-compose.html"><strong aria-hidden="true">3.3.2.</strong> Docker Compose</a></li><li class="chapter-item expanded "><a href="run-with-lambda.html"><strong aria-hidden="true">3.3.3.</strong> AWS Lambda</a></li></ol></li><li class="chapter-item expanded "><a href="run-with-reverse-proxy.html"><strong aria-hidden="true">3.4.</strong> Reverse Proxies</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="run-with-nginx.html"><strong aria-hidden="true">3.4.1.</strong> NGINX</a></li><li class="chapter-item expanded "><a href="run-with-apache.html"><strong aria-hidden="true">3.4.2.</strong> Apache</a></li></ol></li><li class="chapter-item expanded "><a href="troubleshooting.html"><strong aria-hidden="true">3.5.</strong> Troubleshooting</a></li></ol></li><li class="chapter-item expanded "><a href="config-file.html"><strong aria-hidden="true">4.</strong> Configuration File</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="sources-tiles.html"><strong aria-hidden="true">4.1.</strong> Tile sources</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="sources-files.html"><strong aria-hidden="true">4.1.1.</strong> MBTiles and PMTiles File Sources</a></li><li class="chapter-item expanded "><a href="pg-connections.html"><strong aria-hidden="true">4.1.2.</strong> PostgreSQL Connections</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="sources-pg-tables.html"><strong aria-hidden="true">4.1.2.1.</strong> PostgreSQL Table Sources</a></li><li class="chapter-item expanded "><a href="sources-pg-functions.html"><strong aria-hidden="true">4.1.2.2.</strong> PostgreSQL Function Sources</a></li></ol></li><li class="chapter-item expanded "><a href="sources-cog-files.html"><strong aria-hidden="true">4.1.3.</strong> Cloud Optimized GeoTIFF File Sources</a></li><li class="chapter-item expanded "><a href="sources-composite.html"><strong aria-hidden="true">4.1.4.</strong> Composite Sources</a></li></ol></li><li class="chapter-item expanded "><a href="sources-resources.html"><strong aria-hidden="true">4.2.</strong> Supporting Resources</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="sources-sprites.html"><strong aria-hidden="true">4.2.1.</strong> Sprites</a></li><li class="chapter-item expanded "><a href="sources-styles.html"><strong aria-hidden="true">4.2.2.</strong> Styles</a></li><li class="chapter-item expanded "><a href="sources-fonts.html"><strong aria-hidden="true">4.2.3.</strong> Fonts</a></li></ol></li></ol></li><li class="chapter-item expanded "><a href="using.html"><strong aria-hidden="true">5.</strong> Available API Endpoints</a></li><li class="chapter-item expanded "><a href="using-guides.html"><strong aria-hidden="true">6.</strong> Guides</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="using-with-renderer.html"><strong aria-hidden="true">6.1.</strong> Map renderer specific</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="using-with-maplibre.html"><strong aria-hidden="true">6.1.1.</strong> MapLibre</a></li><li class="chapter-item expanded "><a href="using-with-leaflet.html"><strong aria-hidden="true">6.1.2.</strong> Leaflet</a></li><li class="chapter-item expanded "><a href="using-with-deck-gl.html"><strong aria-hidden="true">6.1.3.</strong> deck.gl</a></li><li class="chapter-item expanded "><a href="using-with-mapbox.html"><strong aria-hidden="true">6.1.4.</strong> Mapbox</a></li><li class="chapter-item expanded "><a href="using-with-openlayers.html"><strong aria-hidden="true">6.1.5.</strong> OpenLayers</a></li></ol></li><li class="chapter-item expanded "><a href="using-with-data.html"><strong aria-hidden="true">6.2.</strong> Tile source specific</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="recipes.html"><strong aria-hidden="true">6.2.1.</strong> Using Hosted PostgreSQL</a></li><li class="chapter-item expanded "><a href="recipe-basemap-postgis.html"><strong aria-hidden="true">6.2.2.</strong> Setting up a basemap and overlaying data</a></li><li class="chapter-item expanded "><a href="pg-ssl-certificates.html"><strong aria-hidden="true">6.2.3.</strong> Using PostgreSQL with SSL Certificates</a></li></ol></li><li class="chapter-item expanded "><a href="martin-cp.html"><strong aria-hidden="true">6.3.</strong> Bulk Tile Generation</a></li><li class="chapter-item expanded "><a href="mbtiles.html"><strong aria-hidden="true">6.4.</strong> Working with MBTiles archives</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="mbtiles-schema.html"><strong aria-hidden="true">6.4.1.</strong> MBTiles Schemas</a></li><li class="chapter-item expanded "><a href="mbtiles-meta.html"><strong aria-hidden="true">6.4.2.</strong> Accessing Metadata</a></li><li class="chapter-item expanded "><a href="mbtiles-copy.html"><strong aria-hidden="true">6.4.3.</strong> Copying MBTiles</a></li><li class="chapter-item expanded "><a href="mbtiles-diff.html"><strong aria-hidden="true">6.4.4.</strong> Diffing/Patching MBTiles</a></li><li class="chapter-item expanded "><a href="mbtiles-validation.html"><strong aria-hidden="true">6.4.5.</strong> Validating MBTiles</a></li></ol></li></ol></li><li class="chapter-item expanded "><a href="development.html"><strong aria-hidden="true">7.</strong> Development</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="getting-involved.html"><strong aria-hidden="true">7.1.</strong> Getting involved</a></li><li class="chapter-item expanded "><a href="tools.html"><strong aria-hidden="true">7.2.</strong> Provided Tools</a></li><li class="chapter-item expanded "><a href="martin-as-a-library.html"><strong aria-hidden="true">7.3.</strong> Martin as a library</a></li></ol></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0].split("?")[0];
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);
