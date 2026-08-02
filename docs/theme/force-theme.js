// Force Rust theme — remove any stored theme preference
(function() {
    localStorage.setItem('mdbook-theme', 'rust');
    document.documentElement.classList.remove('light', 'coal', 'navy', 'ayu');
    document.documentElement.classList.add('rust');
})();
