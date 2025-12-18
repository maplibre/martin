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
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="introduction.html">Introduction</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="quick-start.html"><strong aria-hidden="true">1.</strong> Quick Start</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="quick-start-linux.html"><strong aria-hidden="true">1.1.</strong> On Linux</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="quick-start-macos.html"><strong aria-hidden="true">1.2.</strong> On macOS</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="quick-start-windows.html"><strong aria-hidden="true">1.3.</strong> On Windows</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="quick-start-qgis.html"><strong aria-hidden="true">1.4.</strong> View with QGIS</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="installation.html"><strong aria-hidden="true">2.</strong> Installation</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run.html"><strong aria-hidden="true">3.</strong> Running</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-with-cli.html"><strong aria-hidden="true">3.1.</strong> Command Line Interface</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="env-vars.html"><strong aria-hidden="true">3.2.</strong> Environment Variables</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-hosting-environment.html"><strong aria-hidden="true">3.3.</strong> Hosting Environment–specific Guides</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-with-docker.html"><strong aria-hidden="true">3.3.1.</strong> Docker</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-with-docker-compose.html"><strong aria-hidden="true">3.3.2.</strong> Docker Compose</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-with-lambda.html"><strong aria-hidden="true">3.3.3.</strong> AWS Lambda</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-with-reverse-proxy.html"><strong aria-hidden="true">3.4.</strong> Reverse Proxies</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-with-nginx.html"><strong aria-hidden="true">3.4.1.</strong> NGINX</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="run-with-apache.html"><strong aria-hidden="true">3.4.2.</strong> Apache</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="troubleshooting.html"><strong aria-hidden="true">3.5.</strong> Troubleshooting</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="config-file.html"><strong aria-hidden="true">4.</strong> Configuration File</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-tiles.html"><strong aria-hidden="true">4.1.</strong> Tile sources</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-files.html"><strong aria-hidden="true">4.1.1.</strong> MBTiles and PMTiles File Sources</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="pg-connections.html"><strong aria-hidden="true">4.1.2.</strong> PostgreSQL Connections</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-pg-tables.html"><strong aria-hidden="true">4.1.2.1.</strong> PostgreSQL Table Sources</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-pg-functions.html"><strong aria-hidden="true">4.1.2.2.</strong> PostgreSQL Function Sources</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-cog-files.html"><strong aria-hidden="true">4.1.3.</strong> Cloud Optimized GeoTIFF File Sources</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-composite.html"><strong aria-hidden="true">4.1.4.</strong> Composite Sources</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-resources.html"><strong aria-hidden="true">4.2.</strong> Supporting Resources</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-sprites.html"><strong aria-hidden="true">4.2.1.</strong> Sprites</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-styles.html"><strong aria-hidden="true">4.2.2.</strong> Styles</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="sources-fonts.html"><strong aria-hidden="true">4.2.3.</strong> Fonts</a></span></li></ol></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using.html"><strong aria-hidden="true">5.</strong> Available API Endpoints</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-guides.html"><strong aria-hidden="true">6.</strong> Guides</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-with-renderer.html"><strong aria-hidden="true">6.1.</strong> Map renderer specific</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-with-maplibre.html"><strong aria-hidden="true">6.1.1.</strong> MapLibre</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-with-leaflet.html"><strong aria-hidden="true">6.1.2.</strong> Leaflet</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-with-deck-gl.html"><strong aria-hidden="true">6.1.3.</strong> deck.gl</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-with-mapbox.html"><strong aria-hidden="true">6.1.4.</strong> Mapbox</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-with-openlayers.html"><strong aria-hidden="true">6.1.5.</strong> OpenLayers</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="using-with-data.html"><strong aria-hidden="true">6.2.</strong> Tile source specific</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="recipes.html"><strong aria-hidden="true">6.2.1.</strong> Using Hosted PostgreSQL</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="recipe-basemap-postgis.html"><strong aria-hidden="true">6.2.2.</strong> Setting up a basemap and overlaying data</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="pg-ssl-certificates.html"><strong aria-hidden="true">6.2.3.</strong> Using PostgreSQL with SSL Certificates</a></span></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="martin-cp.html"><strong aria-hidden="true">6.3.</strong> Bulk Tile Generation</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="mbtiles.html"><strong aria-hidden="true">6.4.</strong> Working with MBTiles archives</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="mbtiles-schema.html"><strong aria-hidden="true">6.4.1.</strong> MBTiles Schemas</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="mbtiles-meta.html"><strong aria-hidden="true">6.4.2.</strong> Accessing Metadata</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="mbtiles-copy.html"><strong aria-hidden="true">6.4.3.</strong> Copying MBTiles</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="mbtiles-diff.html"><strong aria-hidden="true">6.4.4.</strong> Diffing/Patching MBTiles</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="mbtiles-validation.html"><strong aria-hidden="true">6.4.5.</strong> Validating MBTiles</a></span></li></ol></li></ol><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="architecture.html"><strong aria-hidden="true">7.</strong> Architecture</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="development.html"><strong aria-hidden="true">8.</strong> Development</a></span><ol class="section"><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="getting-involved.html"><strong aria-hidden="true">8.1.</strong> Getting involved</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="tools.html"><strong aria-hidden="true">8.2.</strong> Provided Tools</a></span></li><li class="chapter-item expanded "><span class="chapter-link-wrapper"><a href="martin-as-a-library.html"><strong aria-hidden="true">8.3.</strong> Martin as a library</a></span></li></ol></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split('#')[0].split('?')[0];
        if (current_page.endsWith('/')) {
            current_page += 'index.html';
        }
        const links = Array.prototype.slice.call(this.querySelectorAll('a'));
        const l = links.length;
        for (let i = 0; i < l; ++i) {
            const link = links[i];
            const href = link.getAttribute('href');
            if (href && !href.startsWith('#') && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The 'index' page is supposed to alias the first chapter in the book.
            if (link.href === current_page
                || i === 0
                && path_to_root === ''
                && current_page.endsWith('/index.html')) {
                link.classList.add('active');
                let parent = link.parentElement;
                while (parent) {
                    if (parent.tagName === 'LI' && parent.classList.contains('chapter-item')) {
                        parent.classList.add('expanded');
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', e => {
            if (e.target.tagName === 'A') {
                const clientRect = e.target.getBoundingClientRect();
                const sidebarRect = this.getBoundingClientRect();
                sessionStorage.setItem('sidebar-scroll-offset', clientRect.top - sidebarRect.top);
            }
        }, { passive: true });
        const sidebarScrollOffset = sessionStorage.getItem('sidebar-scroll-offset');
        sessionStorage.removeItem('sidebar-scroll-offset');
        if (sidebarScrollOffset !== null) {
            // preserve sidebar scroll position when navigating via links within sidebar
            const activeSection = this.querySelector('.active');
            if (activeSection) {
                const clientRect = activeSection.getBoundingClientRect();
                const sidebarRect = this.getBoundingClientRect();
                const currentOffset = clientRect.top - sidebarRect.top;
                this.scrollTop += currentOffset - parseFloat(sidebarScrollOffset);
            }
        } else {
            // scroll sidebar to current active section when navigating via
            // 'next/previous chapter' buttons
            const activeSection = document.querySelector('#mdbook-sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        const sidebarAnchorToggles = document.querySelectorAll('.chapter-fold-toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(el => {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define('mdbook-sidebar-scrollbox', MDBookSidebarScrollbox);


// ---------------------------------------------------------------------------
// Support for dynamically adding headers to the sidebar.

(function() {
    // This is used to detect which direction the page has scrolled since the
    // last scroll event.
    let lastKnownScrollPosition = 0;
    // This is the threshold in px from the top of the screen where it will
    // consider a header the "current" header when scrolling down.
    const defaultDownThreshold = 150;
    // Same as defaultDownThreshold, except when scrolling up.
    const defaultUpThreshold = 300;
    // The threshold is a virtual horizontal line on the screen where it
    // considers the "current" header to be above the line. The threshold is
    // modified dynamically to handle headers that are near the bottom of the
    // screen, and to slightly offset the behavior when scrolling up vs down.
    let threshold = defaultDownThreshold;
    // This is used to disable updates while scrolling. This is needed when
    // clicking the header in the sidebar, which triggers a scroll event. It
    // is somewhat finicky to detect when the scroll has finished, so this
    // uses a relatively dumb system of disabling scroll updates for a short
    // time after the click.
    let disableScroll = false;
    // Array of header elements on the page.
    let headers;
    // Array of li elements that are initially collapsed headers in the sidebar.
    // I'm not sure why eslint seems to have a false positive here.
    // eslint-disable-next-line prefer-const
    let headerToggles = [];
    // This is a debugging tool for the threshold which you can enable in the console.
    let thresholdDebug = false;

    // Updates the threshold based on the scroll position.
    function updateThreshold() {
        const scrollTop = window.pageYOffset || document.documentElement.scrollTop;
        const windowHeight = window.innerHeight;
        const documentHeight = document.documentElement.scrollHeight;

        // The number of pixels below the viewport, at most documentHeight.
        // This is used to push the threshold down to the bottom of the page
        // as the user scrolls towards the bottom.
        const pixelsBelow = Math.max(0, documentHeight - (scrollTop + windowHeight));
        // The number of pixels above the viewport, at least defaultDownThreshold.
        // Similar to pixelsBelow, this is used to push the threshold back towards
        // the top when reaching the top of the page.
        const pixelsAbove = Math.max(0, defaultDownThreshold - scrollTop);
        // How much the threshold should be offset once it gets close to the
        // bottom of the page.
        const bottomAdd = Math.max(0, windowHeight - pixelsBelow - defaultDownThreshold);
        let adjustedBottomAdd = bottomAdd;

        // Adjusts bottomAdd for a small document. The calculation above
        // assumes the document is at least twice the windowheight in size. If
        // it is less than that, then bottomAdd needs to be shrunk
        // proportional to the difference in size.
        if (documentHeight < windowHeight * 2) {
            const maxPixelsBelow = documentHeight - windowHeight;
            const t = 1 - pixelsBelow / Math.max(1, maxPixelsBelow);
            const clamp = Math.max(0, Math.min(1, t));
            adjustedBottomAdd *= clamp;
        }

        let scrollingDown = true;
        if (scrollTop < lastKnownScrollPosition) {
            scrollingDown = false;
        }

        if (scrollingDown) {
            // When scrolling down, move the threshold up towards the default
            // downwards threshold position. If near the bottom of the page,
            // adjustedBottomAdd will offset the threshold towards the bottom
            // of the page.
            const amountScrolledDown = scrollTop - lastKnownScrollPosition;
            const adjustedDefault = defaultDownThreshold + adjustedBottomAdd;
            threshold = Math.max(adjustedDefault, threshold - amountScrolledDown);
        } else {
            // When scrolling up, move the threshold down towards the default
            // upwards threshold position. If near the bottom of the page,
            // quickly transition the threshold back up where it normally
            // belongs.
            const amountScrolledUp = lastKnownScrollPosition - scrollTop;
            const adjustedDefault = defaultUpThreshold - pixelsAbove
                + Math.max(0, adjustedBottomAdd - defaultDownThreshold);
            threshold = Math.min(adjustedDefault, threshold + amountScrolledUp);
        }

        if (documentHeight <= windowHeight) {
            threshold = 0;
        }

        if (thresholdDebug) {
            const id = 'mdbook-threshold-debug-data';
            let data = document.getElementById(id);
            if (data === null) {
                data = document.createElement('div');
                data.id = id;
                data.style.cssText = `
                    position: fixed;
                    top: 50px;
                    right: 10px;
                    background-color: 0xeeeeee;
                    z-index: 9999;
                    pointer-events: none;
                `;
                document.body.appendChild(data);
            }
            data.innerHTML = `
                <table>
                  <tr><td>documentHeight</td><td>${documentHeight.toFixed(1)}</td></tr>
                  <tr><td>windowHeight</td><td>${windowHeight.toFixed(1)}</td></tr>
                  <tr><td>scrollTop</td><td>${scrollTop.toFixed(1)}</td></tr>
                  <tr><td>pixelsAbove</td><td>${pixelsAbove.toFixed(1)}</td></tr>
                  <tr><td>pixelsBelow</td><td>${pixelsBelow.toFixed(1)}</td></tr>
                  <tr><td>bottomAdd</td><td>${bottomAdd.toFixed(1)}</td></tr>
                  <tr><td>adjustedBottomAdd</td><td>${adjustedBottomAdd.toFixed(1)}</td></tr>
                  <tr><td>scrollingDown</td><td>${scrollingDown}</td></tr>
                  <tr><td>threshold</td><td>${threshold.toFixed(1)}</td></tr>
                </table>
            `;
            drawDebugLine();
        }

        lastKnownScrollPosition = scrollTop;
    }

    function drawDebugLine() {
        if (!document.body) {
            return;
        }
        const id = 'mdbook-threshold-debug-line';
        const existingLine = document.getElementById(id);
        if (existingLine) {
            existingLine.remove();
        }
        const line = document.createElement('div');
        line.id = id;
        line.style.cssText = `
            position: fixed;
            top: ${threshold}px;
            left: 0;
            width: 100vw;
            height: 2px;
            background-color: red;
            z-index: 9999;
            pointer-events: none;
        `;
        document.body.appendChild(line);
    }

    function mdbookEnableThresholdDebug() {
        thresholdDebug = true;
        updateThreshold();
        drawDebugLine();
    }

    window.mdbookEnableThresholdDebug = mdbookEnableThresholdDebug;

    // Updates which headers in the sidebar should be expanded. If the current
    // header is inside a collapsed group, then it, and all its parents should
    // be expanded.
    function updateHeaderExpanded(currentA) {
        // Add expanded to all header-item li ancestors.
        let current = currentA.parentElement;
        while (current) {
            if (current.tagName === 'LI' && current.classList.contains('header-item')) {
                current.classList.add('expanded');
            }
            current = current.parentElement;
        }
    }

    // Updates which header is marked as the "current" header in the sidebar.
    // This is done with a virtual Y threshold, where headers at or below
    // that line will be considered the current one.
    function updateCurrentHeader() {
        if (!headers || !headers.length) {
            return;
        }

        // Reset the classes, which will be rebuilt below.
        const els = document.getElementsByClassName('current-header');
        for (const el of els) {
            el.classList.remove('current-header');
        }
        for (const toggle of headerToggles) {
            toggle.classList.remove('expanded');
        }

        // Find the last header that is above the threshold.
        let lastHeader = null;
        for (const header of headers) {
            const rect = header.getBoundingClientRect();
            if (rect.top <= threshold) {
                lastHeader = header;
            } else {
                break;
            }
        }
        if (lastHeader === null) {
            lastHeader = headers[0];
            const rect = lastHeader.getBoundingClientRect();
            const windowHeight = window.innerHeight;
            if (rect.top >= windowHeight) {
                return;
            }
        }

        // Get the anchor in the summary.
        const href = '#' + lastHeader.id;
        const a = [...document.querySelectorAll('.header-in-summary')]
            .find(element => element.getAttribute('href') === href);
        if (!a) {
            return;
        }

        a.classList.add('current-header');

        updateHeaderExpanded(a);
    }

    // Updates which header is "current" based on the threshold line.
    function reloadCurrentHeader() {
        if (disableScroll) {
            return;
        }
        updateThreshold();
        updateCurrentHeader();
    }


    // When clicking on a header in the sidebar, this adjusts the threshold so
    // that it is located next to the header. This is so that header becomes
    // "current".
    function headerThresholdClick(event) {
        // See disableScroll description why this is done.
        disableScroll = true;
        setTimeout(() => {
            disableScroll = false;
        }, 100);
        // requestAnimationFrame is used to delay the update of the "current"
        // header until after the scroll is done, and the header is in the new
        // position.
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                // Closest is needed because if it has child elements like <code>.
                const a = event.target.closest('a');
                const href = a.getAttribute('href');
                const targetId = href.substring(1);
                const targetElement = document.getElementById(targetId);
                if (targetElement) {
                    threshold = targetElement.getBoundingClientRect().bottom;
                    updateCurrentHeader();
                }
            });
        });
    }

    // Takes the nodes from the given head and copies them over to the
    // destination, along with some filtering.
    function filterHeader(source, dest) {
        const clone = source.cloneNode(true);
        clone.querySelectorAll('mark').forEach(mark => {
            mark.replaceWith(...mark.childNodes);
        });
        dest.append(...clone.childNodes);
    }

    // Scans page for headers and adds them to the sidebar.
    document.addEventListener('DOMContentLoaded', function() {
        const activeSection = document.querySelector('#mdbook-sidebar .active');
        if (activeSection === null) {
            return;
        }

        const main = document.getElementsByTagName('main')[0];
        headers = Array.from(main.querySelectorAll('h2, h3, h4, h5, h6'))
            .filter(h => h.id !== '' && h.children.length && h.children[0].tagName === 'A');

        if (headers.length === 0) {
            return;
        }

        // Build a tree of headers in the sidebar.

        const stack = [];

        const firstLevel = parseInt(headers[0].tagName.charAt(1));
        for (let i = 1; i < firstLevel; i++) {
            const ol = document.createElement('ol');
            ol.classList.add('section');
            if (stack.length > 0) {
                stack[stack.length - 1].ol.appendChild(ol);
            }
            stack.push({level: i + 1, ol: ol});
        }

        // The level where it will start folding deeply nested headers.
        const foldLevel = 3;

        for (let i = 0; i < headers.length; i++) {
            const header = headers[i];
            const level = parseInt(header.tagName.charAt(1));

            const currentLevel = stack[stack.length - 1].level;
            if (level > currentLevel) {
                // Begin nesting to this level.
                for (let nextLevel = currentLevel + 1; nextLevel <= level; nextLevel++) {
                    const ol = document.createElement('ol');
                    ol.classList.add('section');
                    const last = stack[stack.length - 1];
                    const lastChild = last.ol.lastChild;
                    // Handle the case where jumping more than one nesting
                    // level, which doesn't have a list item to place this new
                    // list inside of.
                    if (lastChild) {
                        lastChild.appendChild(ol);
                    } else {
                        last.ol.appendChild(ol);
                    }
                    stack.push({level: nextLevel, ol: ol});
                }
            } else if (level < currentLevel) {
                while (stack.length > 1 && stack[stack.length - 1].level > level) {
                    stack.pop();
                }
            }

            const li = document.createElement('li');
            li.classList.add('header-item');
            li.classList.add('expanded');
            if (level < foldLevel) {
                li.classList.add('expanded');
            }
            const span = document.createElement('span');
            span.classList.add('chapter-link-wrapper');
            const a = document.createElement('a');
            span.appendChild(a);
            a.href = '#' + header.id;
            a.classList.add('header-in-summary');
            filterHeader(header.children[0], a);
            a.addEventListener('click', headerThresholdClick);
            const nextHeader = headers[i + 1];
            if (nextHeader !== undefined) {
                const nextLevel = parseInt(nextHeader.tagName.charAt(1));
                if (nextLevel > level && level >= foldLevel) {
                    const toggle = document.createElement('a');
                    toggle.classList.add('chapter-fold-toggle');
                    toggle.classList.add('header-toggle');
                    toggle.addEventListener('click', () => {
                        li.classList.toggle('expanded');
                    });
                    const toggleDiv = document.createElement('div');
                    toggleDiv.textContent = '❱';
                    toggle.appendChild(toggleDiv);
                    span.appendChild(toggle);
                    headerToggles.push(li);
                }
            }
            li.appendChild(span);

            const currentParent = stack[stack.length - 1];
            currentParent.ol.appendChild(li);
        }

        const onThisPage = document.createElement('div');
        onThisPage.classList.add('on-this-page');
        onThisPage.append(stack[0].ol);
        const activeItemSpan = activeSection.parentElement;
        activeItemSpan.after(onThisPage);
    });

    document.addEventListener('DOMContentLoaded', reloadCurrentHeader);
    document.addEventListener('scroll', reloadCurrentHeader, { passive: true });
})();

