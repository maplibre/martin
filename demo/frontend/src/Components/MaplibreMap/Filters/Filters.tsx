import { debounce } from "debounce";
import { useCallback, useMemo, useState } from "react";
//@ts-ignore
import { addToRange, type DateRange, DayPicker } from "react-day-picker";
import "react-day-picker/dist/style.css";

import { DEC, JAN } from "../../../config/constants";
import AvgTime from "./AvgTime";
import Container from "./Container";
import DayPickerContainer from "./DayPicker";
import Input from "./Input";
import Layers from "./Layers";
import Range from "./Range";
import Separator from "./Separator";
import TimePicker from "./TimePicker";

interface FiltersProps {
	visibleLayer: string;
	toggleLayer: (layer: string) => void;
	range: DateRange;
	hour: number;
	changeRangeFilter: (value: DateRange) => void;
	changeHourFilter: (value: number) => void;
}

const Filters: React.FC<FiltersProps> = ({
	visibleLayer,
	toggleLayer,
	range,
	hour,
	changeRangeFilter,
	changeHourFilter,
}) => {
	const [isDayPickerEnabled, setIsDayPickerEnabled] = useState(true);

	const debouncedHourChangeFilter = debounce(
		(value: string) => changeHourFilter(parseInt(value)),
		300,
	);

	const isAvgHour = hour === -1;

	return (
		<Container>
			<Layers toggleLayer={toggleLayer} visibleLayer={visibleLayer} />
			<Separator />
			<Range
				onClick={() => setIsDayPickerEnabled((prev) => !prev)}
			>{`${range.from.toLocaleDateString()} – ${range.to.toLocaleDateString()}`}</Range>
			{isDayPickerEnabled && (
				<DayPickerContainer>
					<DayPicker
						mode="range"
						defaultMonth={JAN}
						startMonth={JAN}
						numberOfMonths={1}
						modifiers={range}
						selected={range}
						onSelect={changeRangeFilter}
						style={{ height: "290px",  }}
						endMonth={DEC}
					/>
				</DayPickerContainer>
			)}
			<TimePicker>
				<AvgTime isEnabled={isAvgHour} onClick={() => changeHourFilter(-1)}>
					AVG
				</AvgTime>
				<Input
					max="23"
					min="0"
					onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
						debouncedHourChangeFilter(e.target.value)
					}
					step="1"
					type="range"
					value={hour}
				/>
				{!isAvgHour && (
					<div
						style={{ color: "#DADFEE", fontWeight: "bold" }}
					>{`${hour}:00`}</div>
				)}
			</TimePicker>
		</Container>
	);
};

export default Filters;
