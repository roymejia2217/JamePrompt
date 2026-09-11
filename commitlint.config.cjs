module.exports = {
  extends: ['@commitlint/config-conventional'],
  defaultIgnores: false,
  rules: {
    'body-leading-blank': [2, 'always'],
    'footer-leading-blank': [2, 'always']
  }
};
