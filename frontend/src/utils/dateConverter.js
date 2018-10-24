export default (date) => {
  if (!date) return '';

  return `${date.getDate()}.${date.getMonth() + 1}`;
};
