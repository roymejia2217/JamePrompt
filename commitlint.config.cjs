module.exports = {
  extends: ['@commitlint/config-conventional'],
  defaultIgnores: false,
  rules: {
    'body-empty': [2, 'never'],
    'body-min-length': [2, 'always', 20],
    'body-leading-blank': [2, 'always'],
    'footer-leading-blank': [2, 'always']
  }
};
