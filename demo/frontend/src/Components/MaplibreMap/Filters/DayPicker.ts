import styled from 'styled-components';

export default styled.div`
  border-radius: 5px;
  margin-bottom: 10px;

  background-color: #161626;

  .DayPicker-wrapper {
    outline: none;
  }

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

  .DayPicker-Weekday {
    color: #6C7495;
  }

  .DayPicker-Day--selected:not(.DayPicker-Day--outside) {
    background-color: #2c0ea6 !important;
  }

  .DayPicker-Day:not(
  .DayPicker-Day--disabled):not(
  .DayPicker-Day--selected):not(
  .DayPicker-Day--outside):hover {
    color: #000;
  }

  .DayPicker-Day {
    border-radius: 0 !important;
    outline: none;
  }
`;
