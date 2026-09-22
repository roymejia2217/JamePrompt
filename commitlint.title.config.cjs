const base = require('./commitlint.config.cjs');

module.exports = {
  ...base,
  rules: {
    ...base.rules,
    'body-empty': [0],
    'body-min-length': [0],
    'body-leading-blank': [0],
    'footer-leading-blank': [0]
  }
};
