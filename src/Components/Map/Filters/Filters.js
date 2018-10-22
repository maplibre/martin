import React, { PureComponent } from 'react';
import DayPicker, { DateUtils } from 'react-day-picker';
import { debounce } from 'debounce';
import 'react-day-picker/lib/style.css';

import Container from './Container';
import Title from './Title';
import Description from './Description';
import Range from './Range';
import DayPickerContainer from './DayPicker';
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

  dateConverter = date => date && `${date.getDate()}.${date.getMonth() + 1}`;

  toggleDayPicker = () => {
    this.setState(
      ({ isDayPickerEnabled }) => ({ isDayPickerEnabled: !isDayPickerEnabled })
    );
  };

  render() {
    const { range, hour } = this.props;
    const { from, to } = range;
    const modifiers = { start: from, end: to };

    return (
      <Container>
        <Title>
          Number of trips
        </Title>
        <Description>
          Conducted from an area
        </Description>
        <Range onClick={this.toggleDayPicker}>
          {`${this.dateConverter(from)} – ${this.dateConverter(to)}`}
        </Range>
        {this.state.isDayPickerEnabled && (
          <DayPickerContainer>
            <DayPicker
              numberOfMonths={1}
              selectedDays={[from, { from, to }]}
              modifiers={modifiers}
              onDayClick={this.handleDayClick}
              captionElement={({ date, localeUtils, locale }) => {
                const months = localeUtils.getMonths(locale);

                return (
                  <div className='DayPicker-Caption'>
                    {months[date.getMonth()]}
                  </div>
                );
              }}

              initialMonth={new Date(2017, 0)}
              fromMonth={new Date(2017, 0)}
              toMonth={new Date(2017, 11)}
            />
          </DayPickerContainer>
        )}
        <TimePicker>
          <AvgTime
            isEnabled={hour === -1}
            onClick={() => this.props.changeFilter('hour', -1)}
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
          {hour !== -1 && (
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
