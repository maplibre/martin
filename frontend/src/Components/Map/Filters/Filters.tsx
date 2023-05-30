import React, { PureComponent } from 'react';

//@ts-ignore
import { DayPicker, addToRange } from 'react-day-picker';
import { debounce } from 'debounce';
import 'react-day-picker/dist/style.css';

import { JAN, DEC } from '../../../config/constants';
import dateConverter from '../../../utils/dateConverter';

import Container from './Container';
import Layers from './Layers';
import Separator from './Separator';
import Range from './Range';
import DayPickerContainer from './DayPicker';
import CaptionElement from './CaptionElement';
import TimePicker from './TimePicker';
import AvgTime from './AvgTime';
import Input from './Input';

class Filters extends PureComponent<{visibleLayer, toggleLayer, range, hour, changeFilter}, {isDayPickerEnabled}> {
  constructor(props) {
    super(props);
    this.state = {
      isDayPickerEnabled: true
    };
  }

  handleDayClick = (day) => {
    const range = addToRange(day, this.props.range);

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
    const {
      visibleLayer, toggleLayer, range: { from, to }, hour
    } = this.props;
    const modifiers = {
      start: from,
      end: to
    };
    const isAvgHour = hour === -1;
    const dateFrom = dateConverter(from);
    const dateTo = dateConverter(to);

    return (
      <Container>
        <Layers
          visibleLayer={visibleLayer}
          toggleLayer={toggleLayer}
        />
        <Separator />
        <Range onClick={this.toggleDayPicker}>
          {`${dateFrom} – ${dateTo}`}
        </Range>
        {this.state.isDayPickerEnabled && (
          <DayPickerContainer>
            <DayPicker
              style={{ height: '290px' }}
              numberOfMonths={1}
              selected={[from, { from, to }]}
              modifiers={modifiers}
              onDayClick={this.handleDayClick}
              // captionElement={CaptionElement}
              defaultMonth={JAN}
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
            <div style={{ fontWeight: 'bold', color: '#DADFEE' }}>
              {`${hour}:00`}
            </div>
          )}
        </TimePicker>
      </Container>
    );
  }
}

export default Filters;
