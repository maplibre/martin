import { debounce } from 'debounce';
import { PureComponent } from 'react';
//@ts-ignore
import { addToRange, DayPicker } from 'react-day-picker';
import 'react-day-picker/dist/style.css';

import { DEC, JAN } from '../../../config/constants';
import dateConverter from '../../../utils/dateConverter';
import AvgTime from './AvgTime';
import Container from './Container';
import DayPickerContainer from './DayPicker';
import Input from './Input';
import Layers from './Layers';
import Range from './Range';
import Separator from './Separator';
import TimePicker from './TimePicker';

class Filters extends PureComponent<
  { visibleLayer; toggleLayer; range; hour; changeFilter },
  { isDayPickerEnabled }
> {
  constructor(props) {
    super(props);
    this.state = {
      isDayPickerEnabled: true,
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
    this.setState(({ isDayPickerEnabled }) => ({ isDayPickerEnabled: !isDayPickerEnabled }));
  };

  render() {
    const {
      visibleLayer,
      toggleLayer,
      range: { from, to },
      hour,
    } = this.props;
    const modifiers = {
      end: to,
      start: from,
    };
    const isAvgHour = hour === -1;
    const dateFrom = dateConverter(from);
    const dateTo = dateConverter(to);

    return (
      <Container>
        <Layers toggleLayer={toggleLayer} visibleLayer={visibleLayer} />
        <Separator />
        <Range onClick={this.toggleDayPicker}>{`${dateFrom} – ${dateTo}`}</Range>
        {this.state.isDayPickerEnabled && (
          <DayPickerContainer>
            <DayPicker
              defaultMonth={JAN}
              fromMonth={JAN}
              modifiers={modifiers}
              numberOfMonths={1}
              onDayClick={this.handleDayClick}
              selected={[from, { from, to }]}
              style={{ height: '290px' }}
              toMonth={DEC}
            />
          </DayPickerContainer>
        )}
        <TimePicker>
          <AvgTime $isEnabled={isAvgHour} onClick={this.setAverageTime}>
            AVG
          </AvgTime>
          <Input max="23" min="0" onChange={this.changeTime} step="1" type="range" value={hour} />
          {!isAvgHour && <div style={{ color: '#DADFEE', fontWeight: 'bold' }}>{`${hour}:00`}</div>}
        </TimePicker>
      </Container>
    );
  }
}

export default Filters;
