export default (date) => {
  if (!date) return '';

  return `${date.getMonth() + 1}.${date.getDate()}`;
};
