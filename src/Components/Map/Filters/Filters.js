import React, { PureComponent } from 'react';
import DayPickerInput from 'react-day-picker/DayPickerInput';
import { debounce } from "debounce";
import 'react-day-picker/lib/style.css';

import Container from './Container';
import Title from "./Title";
import Description from "./Description";
import DayPicker from "./DayPicker";
import Input from "./Input";
import TimePicker from "./TimePicker";
import AvgTime from "./AvgTime";

class Filters extends PureComponent {
  changeFrom = (from) => {
    this.props.changeFilter('from', from);
  };

  changeTo = (to) => {
    this.props.changeFilter('to', to);
  };

  changeTime = (e) => {
    debounce(this.props.changeFilter('hour', e.target.value), 300);
  };

  render() {
    const { from, to, hour } = this.props;

    return (
      <Container>
        <Title>
          Number of trips
        </Title>
        <Description>
          The sum of trips for the specified period cho-to mozet escho pro etot pokazatel
        </Description>
        <DayPicker>
          <DayPickerInput
            value={from}
            placeholder="From"
            dayPickerProps={{
              selectedDays: [from, {from, to}],
              disabledDays: {after: to},
              month: new Date(2017, 0),
              numberOfMonths: 1,
              fromMonth: new Date(2017, 0),
              toMonth: new Date(2017, 11),
              onDayClick: () => this.to.getInput().focus(),
            }}
            onDayChange={this.changeFrom}
          />
          {' '}—{' '}
          <span className="InputFromTo-to">
            <DayPickerInput
              ref={el => (this.to = el)}
              value={to}
              placeholder="To"
              dayPickerProps={{
                selectedDays: [from, {from, to}],
                disabledDays: {before: from},
                month: from || new Date(2017, 0),
                fromMonth: new Date(2017, 0),
                toMonth: new Date(2017, 11),
                numberOfMonths: 1,
              }}
              onDayChange={this.changeTo}
            />
          </span>
        </DayPicker>
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
            <div>{hour}:00</div>
          )}
        </TimePicker>
      </Container>
    );
  }
}

export default Filters;
