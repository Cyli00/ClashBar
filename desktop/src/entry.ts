import './style.css';
if (new URLSearchParams(location.search).get('surface') === 'submenu') {
  void import('./menu').then(module => module.mountSubmenu());
} else {
  void import('./main');
}
