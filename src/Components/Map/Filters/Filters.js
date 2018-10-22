import React, { PureComponent } from 'react';
import DayPicker, { DateUtils } from 'react-day-picker';
import { debounce } from 'debounce';
import 'react-day-picker/lib/style.css';

import { JAN, DEC } from '../../../config/constants';
import dateConverter from '../../../utils/dateConverter';

import Container from './Container';
import Title from './Title';
import Description from './Description';
import Separator from './Separator';
import Range from './Range';
import DayPickerContainer from './DayPicker';
import CaptionElement from './CaptionElement';
import TimePicker from './TimePicker';
import AvgTime from './AvgTime';
import Input from './Input';

class Filters extends PureComponent {
  state = {
    isDayPickerEnabled: false
  };

  handleDayClick = (day) => {
    const range = DateUtils.addDayToRange(day, this.props.range);

    this.props.changeFilter('range', range);
  };

  changeTime = (e) => {
    debounce(this.props.changeFilter('hour', e.target.value), 300);
  };

  setAverageTime = () => {
    this.props.changeFilter('hour', -1);
  };

  toggleDayPicker = () => {
    this.setState(
      ({ isDayPickerEnabled }) => ({ isDayPickerEnabled: !isDayPickerEnabled })
    );
  };

  render() {
    const { range: { from, to }, hour } = this.props;
    const modifiers = { start: from, end: to };
    const isAvgHour = hour === -1;
    const dateFrom = dateConverter(from);
    const dateTo = dateConverter(to);

    return (
      <Container>
        <Title>
          Number of trips
        </Title>
        <Description>
          Conducted from an area
        </Description>
        <Separator />
        <Range onClick={this.toggleDayPicker}>
          {`${dateFrom} – ${dateTo}`}
        </Range>
        {this.state.isDayPickerEnabled && (
          <DayPickerContainer>
            <DayPicker
              numberOfMonths={1}
              selectedDays={[from, { from, to }]}
              modifiers={modifiers}
              onDayClick={this.handleDayClick}
              captionElement={CaptionElement}
              initialMonth={JAN}
              fromMonth={JAN}
              toMonth={DEC}
            />
          </DayPickerContainer>
        )}
        <TimePicker>
          <AvgTime
            isEnabled={isAvgHour}
            onClick={this.setAverageTime}
          >
            AVG
          </AvgTime>
          <Input
            type='range'
            value={hour}
            min='0'
            max='23'
            step='1'
            onChange={this.changeTime}
          />
          {!isAvgHour && (
            <div>
              {`${hour}:00`}
            </div>
          )}
        </TimePicker>
      </Container>
    );
  }
}

export default Filters;
