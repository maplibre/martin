import styled from 'styled-components';

export default styled.div`
  border-radius: 5px;
  margin-bottom: 43px;
  
  box-shadow: 0 1px 4px 0 rgba(0, 0, 0, 0.5);
  background-color: #141228;
  
  font-size: 16px !important; 
  
  .DayPicker-Caption > div {
    font-weight: bold;  
    color: #dadfee;
  }
  
  .DayPicker-NavButton--prev {
    margin-right: 0.7em;
  }

  .DayPicker-NavButton {
    width: 0.7em;
    height: 0.7em;
  }

  .DayPicker-Day--selected:not(.DayPicker-Day--outside) {
    background-color: #2c0ea6 !important;
  }

  .DayPicker-Day {
    border-radius: 0 !important;
  }
`;
