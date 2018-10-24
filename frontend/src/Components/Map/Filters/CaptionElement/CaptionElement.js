import React from 'react';

const CaptionElement = ({ date, localeUtils, locale }) => {
  const months = localeUtils.getMonths(locale);

  return (
    <div className='DayPicker-Caption'>
      {months[date.getMonth()]}
    </div>
  );
};

export default CaptionElement;
